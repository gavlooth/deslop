use std::path::Path;

use anyhow::Result;
use deslop_core::Lang;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use tree_sitter::Node;

mod control_flow;
mod resolution;

pub use control_flow::*;
pub use resolution::*;

pub const LANGUAGE_ADAPTER_CAPABILITY_SCHEMA: &str = "deslop.language-adapter-capabilities/2";
pub const CANONICAL_ROLE_SCHEMA: &str = "deslop.canonical-roles/1";
pub const LANGUAGE_QUERY_PACK_SCHEMA: &str = "deslop.language-query-pack/1";
pub const LANGUAGE_LEXICAL_POLICY_SCHEMA: &str = "deslop.language-lexical-policy/1";
pub const LANGUAGE_CONSTRUCT_POLICY_SCHEMA: &str = "deslop.language-construct-policy/1";

/// Portable syntactic categories projected by language adapters.
///
/// Roles are intentionally composable: for example a grammar node may be both a declaration and a
/// callable, or both an expression and a call. Raw grammar kinds and fields remain authoritative
/// grammar evidence and are never replaced by this vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CanonicalRole {
    Project,
    Module,
    Declaration,
    Type,
    Callable,
    Parameter,
    Block,
    Statement,
    Expression,
    Branch,
    Loop,
    Match,
    Case,
    Call,
    Read,
    Write,
    Literal,
    Comment,
    Import,
    Export,
    Error,
    Generated,
    OpaqueRegion,
}

impl CanonicalRole {
    pub const ALL: [Self; 23] = [
        Self::Project,
        Self::Module,
        Self::Declaration,
        Self::Type,
        Self::Callable,
        Self::Parameter,
        Self::Block,
        Self::Statement,
        Self::Expression,
        Self::Branch,
        Self::Loop,
        Self::Match,
        Self::Case,
        Self::Call,
        Self::Read,
        Self::Write,
        Self::Literal,
        Self::Comment,
        Self::Import,
        Self::Export,
        Self::Error,
        Self::Generated,
        Self::OpaqueRegion,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Module => "module",
            Self::Declaration => "declaration",
            Self::Type => "type",
            Self::Callable => "callable",
            Self::Parameter => "parameter",
            Self::Block => "block",
            Self::Statement => "statement",
            Self::Expression => "expression",
            Self::Branch => "branch",
            Self::Loop => "loop",
            Self::Match => "match",
            Self::Case => "case",
            Self::Call => "call",
            Self::Read => "read",
            Self::Write => "write",
            Self::Literal => "literal",
            Self::Comment => "comment",
            Self::Import => "import",
            Self::Export => "export",
            Self::Error => "error",
            Self::Generated => "generated",
            Self::OpaqueRegion => "opaque-region",
        }
    }

    fn catalog_index(self) -> usize {
        Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .expect("the canonical role catalog is exhaustive")
    }

    fn bit(self) -> u32 {
        1_u32 << self.catalog_index()
    }
}

/// A canonical, duplicate-free set of composable roles.
///
/// Its wire form pins both the role schema and catalog order. Construction order does not affect
/// serialization; malformed reordered or duplicate wire roles fail closed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanonicalRoleSet {
    bits: u32,
}

#[derive(Serialize)]
struct CanonicalRoleSetRef<'roles> {
    schema: &'static str,
    roles: &'roles [CanonicalRole],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CanonicalRoleSetWire {
    schema: String,
    roles: Vec<CanonicalRole>,
}

impl CanonicalRoleSet {
    pub fn from_roles(roles: impl IntoIterator<Item = CanonicalRole>) -> Self {
        let mut set = Self::default();
        for role in roles {
            set.insert(role);
        }
        set
    }

    pub fn insert(&mut self, role: CanonicalRole) {
        self.bits |= role.bit();
    }

    pub fn contains(self, role: CanonicalRole) -> bool {
        self.bits & role.bit() != 0
    }

    pub fn is_empty(self) -> bool {
        self.bits == 0
    }

    pub fn len(self) -> usize {
        self.bits.count_ones() as usize
    }

    pub fn union(self, other: Self) -> Self {
        Self {
            bits: self.bits | other.bits,
        }
    }

    pub fn iter(self) -> impl Iterator<Item = CanonicalRole> {
        CanonicalRole::ALL
            .into_iter()
            .filter(move |role| self.contains(*role))
    }
}

impl Serialize for CanonicalRoleSet {
    fn serialize<Serializer>(
        &self,
        serializer: Serializer,
    ) -> std::result::Result<Serializer::Ok, Serializer::Error>
    where
        Serializer: serde::Serializer,
    {
        let roles = self.iter().collect::<Vec<_>>();
        CanonicalRoleSetRef {
            schema: CANONICAL_ROLE_SCHEMA,
            roles: &roles,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CanonicalRoleSet {
    fn deserialize<Deserializer>(
        deserializer: Deserializer,
    ) -> std::result::Result<Self, Deserializer::Error>
    where
        Deserializer: serde::Deserializer<'de>,
    {
        let wire = CanonicalRoleSetWire::deserialize(deserializer)?;
        if wire.schema != CANONICAL_ROLE_SCHEMA {
            return Err(Deserializer::Error::custom(format!(
                "unsupported canonical role schema {}; expected {CANONICAL_ROLE_SCHEMA}",
                wire.schema
            )));
        }
        let mut previous = None;
        for role in &wire.roles {
            let index = role.catalog_index();
            if previous.is_some_and(|previous| index <= previous) {
                return Err(Deserializer::Error::custom(
                    "canonical roles must be unique and in catalog order",
                ));
            }
            previous = Some(index);
        }
        Ok(Self::from_roles(wire.roles))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionSpan {
    pub start_line: usize,
    pub end_line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionClass {
    Behavioral,
    Declaration,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TailPositionClass {
    Return,
    FunctionBody,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SemanticTier {
    #[serde(rename = "S0")]
    S0,
    #[serde(rename = "S1")]
    S1,
    #[serde(rename = "S2")]
    S2,
    #[serde(rename = "S3")]
    S3,
    #[serde(rename = "S4")]
    S4,
}

impl SemanticTier {
    pub const ALL: [Self; 5] = [Self::S0, Self::S1, Self::S2, Self::S3, Self::S4];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AdapterCapability {
    GrammarSelection,
    LosslessSyntax,
    CanonicalRoles,
    SourceSpans,
    Tokens,
    Comments,
    Regions,
    LocalMetrics,
    CloneNormalization,
    SyntacticRecipes,
    LexicalScopes,
    NameResolution,
    ControlFlow,
    DefUse,
    Effects,
    LocalPdg,
    ImportsExports,
    CallGraph,
    DependencyGraph,
    Sdg,
    ApiImpact,
    CompilerTypeEvidence,
    TargetedDynamicVerification,
}

impl AdapterCapability {
    pub const ALL: [Self; 23] = [
        Self::GrammarSelection,
        Self::LosslessSyntax,
        Self::CanonicalRoles,
        Self::SourceSpans,
        Self::Tokens,
        Self::Comments,
        Self::Regions,
        Self::LocalMetrics,
        Self::CloneNormalization,
        Self::SyntacticRecipes,
        Self::LexicalScopes,
        Self::NameResolution,
        Self::ControlFlow,
        Self::DefUse,
        Self::Effects,
        Self::LocalPdg,
        Self::ImportsExports,
        Self::CallGraph,
        Self::DependencyGraph,
        Self::Sdg,
        Self::ApiImpact,
        Self::CompilerTypeEvidence,
        Self::TargetedDynamicVerification,
    ];

    pub const fn tier(self) -> SemanticTier {
        match self {
            Self::GrammarSelection
            | Self::LosslessSyntax
            | Self::CanonicalRoles
            | Self::SourceSpans
            | Self::Tokens
            | Self::Comments => SemanticTier::S0,
            Self::Regions
            | Self::LocalMetrics
            | Self::CloneNormalization
            | Self::SyntacticRecipes => SemanticTier::S1,
            Self::LexicalScopes
            | Self::NameResolution
            | Self::ControlFlow
            | Self::DefUse
            | Self::Effects
            | Self::LocalPdg => SemanticTier::S2,
            Self::ImportsExports
            | Self::CallGraph
            | Self::DependencyGraph
            | Self::Sdg
            | Self::ApiImpact => SemanticTier::S3,
            Self::CompilerTypeEvidence | Self::TargetedDynamicVerification => SemanticTier::S4,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GrammarSelection => "grammar-selection",
            Self::LosslessSyntax => "lossless-syntax",
            Self::CanonicalRoles => "canonical-roles",
            Self::SourceSpans => "source-spans",
            Self::Tokens => "tokens",
            Self::Comments => "comments",
            Self::Regions => "regions",
            Self::LocalMetrics => "local-metrics",
            Self::CloneNormalization => "clone-normalization",
            Self::SyntacticRecipes => "syntactic-recipes",
            Self::LexicalScopes => "lexical-scopes",
            Self::NameResolution => "name-resolution",
            Self::ControlFlow => "control-flow",
            Self::DefUse => "def-use",
            Self::Effects => "effects",
            Self::LocalPdg => "local-pdg",
            Self::ImportsExports => "imports-exports",
            Self::CallGraph => "call-graph",
            Self::DependencyGraph => "dependency-graph",
            Self::Sdg => "sdg",
            Self::ApiImpact => "api-impact",
            Self::CompilerTypeEvidence => "compiler-type-evidence",
            Self::TargetedDynamicVerification => "targeted-dynamic-verification",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilitySupport {
    Provided,
    Unsupported,
    Unknown,
}

impl CapabilitySupport {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Provided => "provided",
            Self::Unsupported => "unsupported",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityAuthority {
    Syntax,
    Adapter,
    LanguageServer,
    Compiler,
    RuntimeVerification,
}

impl CapabilityAuthority {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::Adapter => "adapter",
            Self::LanguageServer => "language-server",
            Self::Compiler => "compiler",
            Self::RuntimeVerification => "runtime-verification",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityDeclaration {
    capability: AdapterCapability,
    support: CapabilitySupport,
    authority: Option<CapabilityAuthority>,
}

impl CapabilityDeclaration {
    pub const fn provided(capability: AdapterCapability, authority: CapabilityAuthority) -> Self {
        Self {
            capability,
            support: CapabilitySupport::Provided,
            authority: Some(authority),
        }
    }

    pub const fn unsupported(capability: AdapterCapability) -> Self {
        Self {
            capability,
            support: CapabilitySupport::Unsupported,
            authority: None,
        }
    }

    pub const fn unknown(capability: AdapterCapability) -> Self {
        Self {
            capability,
            support: CapabilitySupport::Unknown,
            authority: None,
        }
    }

    pub fn capability(&self) -> AdapterCapability {
        self.capability
    }

    pub fn support(&self) -> CapabilitySupport {
        self.support
    }

    pub fn authority(&self) -> Option<CapabilityAuthority> {
        self.authority
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LanguageAdapterCapabilityManifest {
    schema: String,
    adapter_schema: String,
    capabilities: Vec<CapabilityDeclaration>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LanguageAdapterCapabilityManifestWire {
    schema: String,
    adapter_schema: String,
    capabilities: Vec<CapabilityDeclaration>,
}

impl<'de> Deserialize<'de> for LanguageAdapterCapabilityManifest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = LanguageAdapterCapabilityManifestWire::deserialize(deserializer)?;
        let manifest = Self {
            schema: wire.schema,
            adapter_schema: wire.adapter_schema,
            capabilities: wire.capabilities,
        };
        manifest.validate().map_err(D::Error::custom)?;
        Ok(manifest)
    }
}

impl LanguageAdapterCapabilityManifest {
    pub fn new(
        adapter_schema: impl Into<String>,
        capabilities: Vec<CapabilityDeclaration>,
    ) -> Result<Self, String> {
        let manifest = Self {
            schema: LANGUAGE_ADAPTER_CAPABILITY_SCHEMA.to_string(),
            adapter_schema: adapter_schema.into(),
            capabilities,
        };
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn unknown(adapter_schema: impl Into<String>) -> Self {
        Self::new(
            adapter_schema,
            AdapterCapability::ALL
                .into_iter()
                .map(CapabilityDeclaration::unknown)
                .collect(),
        )
        .expect("the total unknown manifest is valid")
    }

    pub fn current_syntax(adapter_schema: impl Into<String>) -> Self {
        Self::new(
            adapter_schema,
            AdapterCapability::ALL
                .into_iter()
                .map(|capability| match capability {
                    AdapterCapability::GrammarSelection
                    | AdapterCapability::LosslessSyntax
                    | AdapterCapability::SourceSpans
                    | AdapterCapability::Tokens => {
                        CapabilityDeclaration::provided(capability, CapabilityAuthority::Syntax)
                    }
                    AdapterCapability::Comments
                    | AdapterCapability::Regions
                    | AdapterCapability::LocalMetrics
                    | AdapterCapability::CloneNormalization
                    | AdapterCapability::SyntacticRecipes => {
                        CapabilityDeclaration::provided(capability, CapabilityAuthority::Adapter)
                    }
                    _ => CapabilityDeclaration::unknown(capability),
                })
                .collect(),
        )
        .expect("the current syntax manifest is valid")
    }

    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn adapter_schema(&self) -> &str {
        &self.adapter_schema
    }

    pub fn capabilities(&self) -> &[CapabilityDeclaration] {
        &self.capabilities
    }

    pub fn declaration(&self, capability: AdapterCapability) -> &CapabilityDeclaration {
        let index = AdapterCapability::ALL
            .iter()
            .position(|candidate| *candidate == capability)
            .expect("the capability catalog is exhaustive");
        &self.capabilities[index]
    }

    pub fn with_declaration(mut self, declaration: CapabilityDeclaration) -> Result<Self, String> {
        let index = AdapterCapability::ALL
            .iter()
            .position(|capability| *capability == declaration.capability)
            .expect("the capability catalog is exhaustive");
        self.capabilities[index] = declaration;
        self.validate()?;
        Ok(self)
    }

    pub fn highest_complete_tier(&self) -> Option<SemanticTier> {
        SemanticTier::ALL
            .into_iter()
            .take_while(|tier| {
                self.capabilities.iter().all(|declaration| {
                    declaration.capability.tier() > *tier
                        || declaration.support == CapabilitySupport::Provided
                })
            })
            .last()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != LANGUAGE_ADAPTER_CAPABILITY_SCHEMA {
            return Err(format!(
                "unsupported adapter capability schema {}",
                self.schema
            ));
        }
        if self.adapter_schema.trim().is_empty() {
            return Err("adapter schema must not be empty".to_string());
        }
        if self.capabilities.len() != AdapterCapability::ALL.len() {
            return Err(format!(
                "adapter capability manifest has {} declarations; expected {}",
                self.capabilities.len(),
                AdapterCapability::ALL.len()
            ));
        }
        for (expected, declaration) in AdapterCapability::ALL.iter().zip(&self.capabilities) {
            if declaration.capability != *expected {
                return Err(format!(
                    "adapter capability declaration order is not total at {expected:?}"
                ));
            }
            match (declaration.support, declaration.authority) {
                (CapabilitySupport::Provided, Some(_))
                | (CapabilitySupport::Unsupported | CapabilitySupport::Unknown, None) => {}
                (CapabilitySupport::Provided, None) => {
                    return Err(format!(
                        "provided capability {:?} has no authority",
                        declaration.capability
                    ));
                }
                (CapabilitySupport::Unsupported | CapabilitySupport::Unknown, Some(_)) => {
                    return Err(format!(
                        "unavailable capability {:?} claims authority",
                        declaration.capability
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QueryFamily {
    Declarations,
    References,
    Scopes,
    Control,
    Comments,
    OpaqueGenerated,
    /// Contract facts for refactor-defect detection
    /// (`docs/REFACTOR_DEFECT_ACCUMULATION.md`): candidate owner/consumer
    /// functions, call/attribute references, schema literals, config reads,
    /// loop constructs, assertion statements, and whole call expressions.
    /// Adapters that cannot support the family declare it unknown, which
    /// surfaces as a per-language capability gap, never a silent absence.
    Contract,
}

impl QueryFamily {
    pub const ALL: [Self; 7] = [
        Self::Declarations,
        Self::References,
        Self::Scopes,
        Self::Control,
        Self::Comments,
        Self::OpaqueGenerated,
        Self::Contract,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Declarations => "declarations",
            Self::References => "references",
            Self::Scopes => "scopes",
            Self::Control => "control",
            Self::Comments => "comments",
            Self::OpaqueGenerated => "opaque-generated",
            Self::Contract => "contract",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryCaptureDeclaration {
    name: String,
    roles: CanonicalRoleSet,
}

impl QueryCaptureDeclaration {
    pub fn new(name: impl Into<String>, roles: CanonicalRoleSet) -> Result<Self, String> {
        let declaration = Self {
            name: name.into(),
            roles,
        };
        declaration.validate()?;
        Ok(declaration)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn roles(&self) -> CanonicalRoleSet {
        self.roles
    }

    fn validate(&self) -> Result<(), String> {
        if self.name.is_empty()
            || !self.name.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'_' | b'-' | b'.')
            })
            || !self.name.as_bytes()[0].is_ascii_lowercase()
        {
            return Err(format!(
                "query capture name {:?} is not canonical lowercase syntax",
                self.name
            ));
        }
        if self.roles.is_empty() {
            return Err(format!("query capture {} has no canonical role", self.name));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryFamilyDeclaration {
    family: QueryFamily,
    support: CapabilitySupport,
    authority: Option<CapabilityAuthority>,
    source: Option<String>,
    captures: Vec<QueryCaptureDeclaration>,
}

impl QueryFamilyDeclaration {
    pub fn provided(
        family: QueryFamily,
        authority: CapabilityAuthority,
        source: impl Into<String>,
        captures: Vec<QueryCaptureDeclaration>,
    ) -> Self {
        Self {
            family,
            support: CapabilitySupport::Provided,
            authority: Some(authority),
            source: Some(source.into()),
            captures,
        }
    }

    pub const fn unsupported(family: QueryFamily) -> Self {
        Self {
            family,
            support: CapabilitySupport::Unsupported,
            authority: None,
            source: None,
            captures: Vec::new(),
        }
    }

    pub const fn unknown(family: QueryFamily) -> Self {
        Self {
            family,
            support: CapabilitySupport::Unknown,
            authority: None,
            source: None,
            captures: Vec::new(),
        }
    }

    pub fn family(&self) -> QueryFamily {
        self.family
    }

    pub fn support(&self) -> CapabilitySupport {
        self.support
    }

    pub fn authority(&self) -> Option<CapabilityAuthority> {
        self.authority
    }

    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    pub fn captures(&self) -> &[QueryCaptureDeclaration] {
        &self.captures
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LanguageQueryPack {
    schema: String,
    adapter_schema: String,
    queries: Vec<QueryFamilyDeclaration>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LanguageQueryPackWire {
    schema: String,
    adapter_schema: String,
    queries: Vec<QueryFamilyDeclaration>,
}

impl<'de> Deserialize<'de> for LanguageQueryPack {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = LanguageQueryPackWire::deserialize(deserializer)?;
        let pack = Self {
            schema: wire.schema,
            adapter_schema: wire.adapter_schema,
            queries: wire.queries,
        };
        pack.validate().map_err(D::Error::custom)?;
        Ok(pack)
    }
}

impl LanguageQueryPack {
    pub fn new(
        adapter_schema: impl Into<String>,
        queries: Vec<QueryFamilyDeclaration>,
    ) -> Result<Self, String> {
        let pack = Self {
            schema: LANGUAGE_QUERY_PACK_SCHEMA.to_string(),
            adapter_schema: adapter_schema.into(),
            queries,
        };
        pack.validate()?;
        Ok(pack)
    }

    pub fn unknown(adapter_schema: impl Into<String>) -> Self {
        Self::new(
            adapter_schema,
            QueryFamily::ALL
                .into_iter()
                .map(QueryFamilyDeclaration::unknown)
                .collect(),
        )
        .expect("the total unknown query pack is valid")
    }

    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn adapter_schema(&self) -> &str {
        &self.adapter_schema
    }

    pub fn queries(&self) -> &[QueryFamilyDeclaration] {
        &self.queries
    }

    pub fn declaration(&self, family: QueryFamily) -> &QueryFamilyDeclaration {
        let index = QueryFamily::ALL
            .iter()
            .position(|candidate| *candidate == family)
            .expect("the query family catalog is exhaustive");
        &self.queries[index]
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != LANGUAGE_QUERY_PACK_SCHEMA {
            return Err(format!(
                "unsupported language query pack schema {}",
                self.schema
            ));
        }
        if self.adapter_schema.trim().is_empty() {
            return Err("query pack adapter schema must not be empty".to_string());
        }
        if self.queries.len() != QueryFamily::ALL.len() {
            return Err(format!(
                "language query pack has {} declarations; expected {}",
                self.queries.len(),
                QueryFamily::ALL.len()
            ));
        }
        for (expected, declaration) in QueryFamily::ALL.iter().zip(&self.queries) {
            if declaration.family != *expected {
                return Err(format!(
                    "language query declaration order is not total at {expected:?}"
                ));
            }
            match declaration.support {
                CapabilitySupport::Provided => {
                    if declaration.authority.is_none() {
                        return Err(format!(
                            "provided {:?} query has no authority",
                            declaration.family
                        ));
                    }
                    if declaration
                        .source
                        .as_ref()
                        .is_none_or(|source| source.trim().is_empty())
                    {
                        return Err(format!(
                            "provided {:?} query has no source",
                            declaration.family
                        ));
                    }
                    if declaration.captures.is_empty() {
                        return Err(format!(
                            "provided {:?} query has no capture declarations",
                            declaration.family
                        ));
                    }
                    let mut names = std::collections::BTreeSet::new();
                    for capture in &declaration.captures {
                        capture.validate()?;
                        if !names.insert(&capture.name) {
                            return Err(format!(
                                "provided {:?} query repeats capture {}",
                                declaration.family, capture.name
                            ));
                        }
                    }
                }
                CapabilitySupport::Unsupported | CapabilitySupport::Unknown => {
                    if declaration.authority.is_some()
                        || declaration.source.is_some()
                        || !declaration.captures.is_empty()
                    {
                        return Err(format!(
                            "unavailable {:?} query retains provided payload",
                            declaration.family
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LexicalTokenClass {
    Identifier,
    Keyword,
    Literal,
    Operator,
    Delimiter,
    Punctuation,
    Comment,
    Error,
    Other,
}

impl LexicalTokenClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Identifier => "identifier",
            Self::Keyword => "keyword",
            Self::Literal => "literal",
            Self::Operator => "operator",
            Self::Delimiter => "delimiter",
            Self::Punctuation => "punctuation",
            Self::Comment => "comment",
            Self::Error => "error",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LexicalOperatorClass {
    Arithmetic,
    Comparison,
    Logical,
    Assignment,
    Bitwise,
    MemberAccess,
    Range,
    Other,
}

impl LexicalOperatorClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Arithmetic => "arithmetic",
            Self::Comparison => "comparison",
            Self::Logical => "logical",
            Self::Assignment => "assignment",
            Self::Bitwise => "bitwise",
            Self::MemberAccess => "member-access",
            Self::Range => "range",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IdentifierCasePolicy {
    Sensitive,
    Insensitive,
    Contextual,
}

impl IdentifierCasePolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sensitive => "sensitive",
            Self::Insensitive => "insensitive",
            Self::Contextual => "contextual",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockCommentDelimiter {
    open: String,
    close: String,
    nested: bool,
}

impl BlockCommentDelimiter {
    pub fn new(open: impl Into<String>, close: impl Into<String>, nested: bool) -> Self {
        Self {
            open: open.into(),
            close: close.into(),
            nested,
        }
    }

    pub fn open(&self) -> &str {
        &self.open
    }

    pub fn close(&self) -> &str {
        &self.close
    }

    pub fn nested(&self) -> bool {
        self.nested
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LexicalClassification {
    token: LexicalTokenClass,
    operator: Option<LexicalOperatorClass>,
}

impl LexicalClassification {
    pub const fn token(token: LexicalTokenClass) -> Self {
        Self {
            token,
            operator: None,
        }
    }

    pub const fn operator(operator: LexicalOperatorClass) -> Self {
        Self {
            token: LexicalTokenClass::Operator,
            operator: Some(operator),
        }
    }

    pub fn token_class(&self) -> LexicalTokenClass {
        self.token
    }

    pub fn operator_class(&self) -> Option<LexicalOperatorClass> {
        self.operator
    }

    fn validate(&self) -> Result<(), String> {
        match (self.token, self.operator) {
            (LexicalTokenClass::Operator, Some(_))
            | (
                LexicalTokenClass::Identifier
                | LexicalTokenClass::Keyword
                | LexicalTokenClass::Literal
                | LexicalTokenClass::Delimiter
                | LexicalTokenClass::Punctuation
                | LexicalTokenClass::Comment
                | LexicalTokenClass::Error
                | LexicalTokenClass::Other,
                None,
            ) => Ok(()),
            (LexicalTokenClass::Operator, None) => {
                Err("operator token has no operator class".to_string())
            }
            (_, Some(_)) => Err("non-operator token claims an operator class".to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LexicalRule {
    raw_kind: String,
    text: Option<String>,
    classification: LexicalClassification,
}

impl LexicalRule {
    pub fn new(
        raw_kind: impl Into<String>,
        text: Option<String>,
        classification: LexicalClassification,
    ) -> Self {
        Self {
            raw_kind: raw_kind.into(),
            text,
            classification,
        }
    }

    pub fn raw_kind(&self) -> &str {
        &self.raw_kind
    }

    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    pub fn classification(&self) -> &LexicalClassification {
        &self.classification
    }

    fn matches(&self, raw_kind: &str, text: &str) -> bool {
        ((self.raw_kind == "*" && self.text.is_none()) || self.raw_kind == raw_kind)
            && self.text.as_deref().is_none_or(|expected| expected == text)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LanguageLexicalPolicy {
    schema: String,
    adapter_schema: String,
    support: CapabilitySupport,
    authority: Option<CapabilityAuthority>,
    identifier_case: Option<IdentifierCasePolicy>,
    unicode_identifiers: Option<bool>,
    line_comments: Vec<String>,
    block_comments: Vec<BlockCommentDelimiter>,
    rules: Vec<LexicalRule>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LanguageLexicalPolicyWire {
    schema: String,
    adapter_schema: String,
    support: CapabilitySupport,
    authority: Option<CapabilityAuthority>,
    identifier_case: Option<IdentifierCasePolicy>,
    unicode_identifiers: Option<bool>,
    line_comments: Vec<String>,
    block_comments: Vec<BlockCommentDelimiter>,
    rules: Vec<LexicalRule>,
}

impl<'de> Deserialize<'de> for LanguageLexicalPolicy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = LanguageLexicalPolicyWire::deserialize(deserializer)?;
        let policy = Self {
            schema: wire.schema,
            adapter_schema: wire.adapter_schema,
            support: wire.support,
            authority: wire.authority,
            identifier_case: wire.identifier_case,
            unicode_identifiers: wire.unicode_identifiers,
            line_comments: wire.line_comments,
            block_comments: wire.block_comments,
            rules: wire.rules,
        };
        policy.validate().map_err(D::Error::custom)?;
        Ok(policy)
    }
}

impl LanguageLexicalPolicy {
    pub fn unknown(adapter_schema: impl Into<String>) -> Self {
        Self {
            schema: LANGUAGE_LEXICAL_POLICY_SCHEMA.to_string(),
            adapter_schema: adapter_schema.into(),
            support: CapabilitySupport::Unknown,
            authority: None,
            identifier_case: None,
            unicode_identifiers: None,
            line_comments: Vec::new(),
            block_comments: Vec::new(),
            rules: Vec::new(),
        }
    }

    pub fn unsupported(adapter_schema: impl Into<String>) -> Self {
        let mut policy = Self::unknown(adapter_schema);
        policy.support = CapabilitySupport::Unsupported;
        policy
    }

    #[allow(clippy::too_many_arguments)]
    pub fn provided(
        adapter_schema: impl Into<String>,
        authority: CapabilityAuthority,
        identifier_case: IdentifierCasePolicy,
        unicode_identifiers: bool,
        line_comments: Vec<String>,
        block_comments: Vec<BlockCommentDelimiter>,
        rules: Vec<LexicalRule>,
    ) -> Result<Self, String> {
        let policy = Self {
            schema: LANGUAGE_LEXICAL_POLICY_SCHEMA.to_string(),
            adapter_schema: adapter_schema.into(),
            support: CapabilitySupport::Provided,
            authority: Some(authority),
            identifier_case: Some(identifier_case),
            unicode_identifiers: Some(unicode_identifiers),
            line_comments,
            block_comments,
            rules,
        };
        policy.validate()?;
        Ok(policy)
    }

    pub fn schema(&self) -> &str {
        &self.schema
    }
    pub fn adapter_schema(&self) -> &str {
        &self.adapter_schema
    }
    pub fn support(&self) -> CapabilitySupport {
        self.support
    }
    pub fn authority(&self) -> Option<CapabilityAuthority> {
        self.authority
    }
    pub fn identifier_case(&self) -> Option<IdentifierCasePolicy> {
        self.identifier_case
    }
    pub fn unicode_identifiers(&self) -> Option<bool> {
        self.unicode_identifiers
    }
    pub fn line_comments(&self) -> &[String] {
        &self.line_comments
    }
    pub fn block_comments(&self) -> &[BlockCommentDelimiter] {
        &self.block_comments
    }
    pub fn rules(&self) -> &[LexicalRule] {
        &self.rules
    }

    pub fn classify(&self, raw_kind: &str, text: &str) -> Option<&LexicalClassification> {
        (self.support == CapabilitySupport::Provided)
            .then(|| self.rules.iter().find(|rule| rule.matches(raw_kind, text)))
            .flatten()
            .map(LexicalRule::classification)
    }

    /// Match only an adapter-declared rule, excluding the required terminal wildcard fallback.
    pub fn classify_explicit(&self, raw_kind: &str, text: &str) -> Option<&LexicalClassification> {
        (self.support == CapabilitySupport::Provided)
            .then(|| {
                self.rules
                    .get(..self.rules.len().saturating_sub(1))?
                    .iter()
                    .find(|rule| rule.matches(raw_kind, text))
            })
            .flatten()
            .map(LexicalRule::classification)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != LANGUAGE_LEXICAL_POLICY_SCHEMA {
            return Err(format!(
                "unsupported language lexical policy schema {}",
                self.schema
            ));
        }
        if self.adapter_schema.trim().is_empty() {
            return Err("lexical policy adapter schema must not be empty".to_string());
        }
        match self.support {
            CapabilitySupport::Provided => {
                if self.authority.is_none()
                    || self.identifier_case.is_none()
                    || self.unicode_identifiers.is_none()
                {
                    return Err(
                        "provided lexical policy lacks authority or identifier policy".to_string(),
                    );
                }
                if self
                    .rules
                    .last()
                    .is_none_or(|rule| rule.raw_kind != "*" || rule.text.is_some())
                {
                    return Err(
                        "provided lexical policy lacks a terminal wildcard fallback".to_string()
                    );
                }
                let mut keys = std::collections::BTreeSet::new();
                for (index, rule) in self.rules.iter().enumerate() {
                    if rule.raw_kind.trim().is_empty() || !keys.insert((&rule.raw_kind, &rule.text))
                    {
                        return Err(
                            "lexical rules have an empty or duplicate match key".to_string()
                        );
                    }
                    if rule.raw_kind == "*" && rule.text.is_none() && index + 1 != self.rules.len()
                    {
                        return Err("lexical wildcard rule must be terminal".to_string());
                    }
                    if rule.text.is_none()
                        && self.rules[index + 1..]
                            .iter()
                            .any(|later| later.raw_kind == rule.raw_kind)
                    {
                        return Err(format!(
                            "lexical kind fallback {} shadows a later exact-text rule",
                            rule.raw_kind
                        ));
                    }
                    rule.classification.validate()?;
                }
                if self.line_comments.iter().any(String::is_empty)
                    || self
                        .block_comments
                        .iter()
                        .any(|delimiter| delimiter.open.is_empty() || delimiter.close.is_empty())
                {
                    return Err("lexical comment delimiters must not be empty".to_string());
                }
            }
            CapabilitySupport::Unsupported | CapabilitySupport::Unknown => {
                if self.authority.is_some()
                    || self.identifier_case.is_some()
                    || self.unicode_identifiers.is_some()
                    || !self.line_comments.is_empty()
                    || !self.block_comments.is_empty()
                    || !self.rules.is_empty()
                {
                    return Err("unavailable lexical policy retains provided payload".to_string());
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ParseRecoveryHandling {
    FileIncomplete,
}

impl ParseRecoveryHandling {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FileIncomplete => "file-incomplete",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParseRecoveryPolicy {
    support: CapabilitySupport,
    authority: Option<CapabilityAuthority>,
    handling: Option<ParseRecoveryHandling>,
}

impl ParseRecoveryPolicy {
    pub const fn provided(authority: CapabilityAuthority, handling: ParseRecoveryHandling) -> Self {
        Self {
            support: CapabilitySupport::Provided,
            authority: Some(authority),
            handling: Some(handling),
        }
    }

    pub const fn unsupported() -> Self {
        Self {
            support: CapabilitySupport::Unsupported,
            authority: None,
            handling: None,
        }
    }

    pub const fn unknown() -> Self {
        Self {
            support: CapabilitySupport::Unknown,
            authority: None,
            handling: None,
        }
    }

    pub fn support(&self) -> CapabilitySupport {
        self.support
    }

    pub fn authority(&self) -> Option<CapabilityAuthority> {
        self.authority
    }

    pub fn handling(&self) -> Option<ParseRecoveryHandling> {
        self.handling
    }

    fn validate(&self) -> Result<(), String> {
        match self.support {
            CapabilitySupport::Provided if self.authority.is_some() && self.handling.is_some() => {
                Ok(())
            }
            CapabilitySupport::Provided => {
                Err("provided parse-recovery policy lacks authority or handling".to_string())
            }
            CapabilitySupport::Unsupported | CapabilitySupport::Unknown
                if self.authority.is_none() && self.handling.is_none() =>
            {
                Ok(())
            }
            CapabilitySupport::Unsupported | CapabilitySupport::Unknown => {
                Err("unavailable parse-recovery policy retains provided payload".to_string())
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConstructPolicyKind {
    UnsupportedConstruct,
    Macro,
    GeneratedCode,
}

impl ConstructPolicyKind {
    pub const ALL: [Self; 3] = [Self::UnsupportedConstruct, Self::Macro, Self::GeneratedCode];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedConstruct => "unsupported-construct",
            Self::Macro => "macro",
            Self::GeneratedCode => "generated-code",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConstructHandling {
    Opaque,
    SurfaceSyntax,
}

impl ConstructHandling {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Opaque => "opaque",
            Self::SurfaceSyntax => "surface-syntax",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConstructRule {
    raw_kind: String,
    text: Option<String>,
    handling: ConstructHandling,
}

impl ConstructRule {
    pub fn new(
        raw_kind: impl Into<String>,
        text: Option<String>,
        handling: ConstructHandling,
    ) -> Self {
        Self {
            raw_kind: raw_kind.into(),
            text,
            handling,
        }
    }

    pub fn raw_kind(&self) -> &str {
        &self.raw_kind
    }

    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    pub fn handling(&self) -> ConstructHandling {
        self.handling
    }

    fn matches(&self, raw_kind: &str, text: &str) -> bool {
        self.raw_kind == raw_kind && self.text.as_deref().is_none_or(|expected| expected == text)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConstructPolicySection {
    kind: ConstructPolicyKind,
    support: CapabilitySupport,
    authority: Option<CapabilityAuthority>,
    rules: Vec<ConstructRule>,
}

impl ConstructPolicySection {
    pub fn provided(
        kind: ConstructPolicyKind,
        authority: CapabilityAuthority,
        rules: Vec<ConstructRule>,
    ) -> Result<Self, String> {
        let section = Self {
            kind,
            support: CapabilitySupport::Provided,
            authority: Some(authority),
            rules,
        };
        section.validate()?;
        Ok(section)
    }

    pub const fn unsupported(kind: ConstructPolicyKind) -> Self {
        Self {
            kind,
            support: CapabilitySupport::Unsupported,
            authority: None,
            rules: Vec::new(),
        }
    }

    pub const fn unknown(kind: ConstructPolicyKind) -> Self {
        Self {
            kind,
            support: CapabilitySupport::Unknown,
            authority: None,
            rules: Vec::new(),
        }
    }

    pub fn kind(&self) -> ConstructPolicyKind {
        self.kind
    }

    pub fn support(&self) -> CapabilitySupport {
        self.support
    }

    pub fn authority(&self) -> Option<CapabilityAuthority> {
        self.authority
    }

    pub fn rules(&self) -> &[ConstructRule] {
        &self.rules
    }

    pub fn matching_rule(&self, raw_kind: &str, text: &str) -> Option<&ConstructRule> {
        (self.support == CapabilitySupport::Provided)
            .then(|| self.rules.iter().find(|rule| rule.matches(raw_kind, text)))
            .flatten()
    }

    fn validate(&self) -> Result<(), String> {
        match self.support {
            CapabilitySupport::Provided => {
                if self.authority.is_none() || self.rules.is_empty() {
                    return Err(format!(
                        "provided {} policy lacks authority or rules",
                        self.kind.as_str()
                    ));
                }
                let mut keys = std::collections::BTreeSet::new();
                for (index, rule) in self.rules.iter().enumerate() {
                    if rule.raw_kind.trim().is_empty()
                        || rule.raw_kind == "*"
                        || !keys.insert((&rule.raw_kind, &rule.text))
                    {
                        return Err(format!(
                            "{} rules have an empty, wildcard, or duplicate match key",
                            self.kind.as_str()
                        ));
                    }
                    if rule.text.is_none()
                        && self.rules[index + 1..]
                            .iter()
                            .any(|later| later.raw_kind == rule.raw_kind)
                    {
                        return Err(format!(
                            "{} kind fallback {} shadows a later exact-text rule",
                            self.kind.as_str(),
                            rule.raw_kind
                        ));
                    }
                }
            }
            CapabilitySupport::Unsupported | CapabilitySupport::Unknown => {
                if self.authority.is_some() || !self.rules.is_empty() {
                    return Err(format!(
                        "unavailable {} policy retains provided payload",
                        self.kind.as_str()
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DialectDeclaration {
    dialect: String,
    grammar_id: String,
    grammar_version: String,
}

impl DialectDeclaration {
    pub fn new(
        dialect: impl Into<String>,
        grammar_id: impl Into<String>,
        grammar_version: impl Into<String>,
    ) -> Self {
        Self {
            dialect: dialect.into(),
            grammar_id: grammar_id.into(),
            grammar_version: grammar_version.into(),
        }
    }

    pub fn dialect(&self) -> &str {
        &self.dialect
    }

    pub fn grammar_id(&self) -> &str {
        &self.grammar_id
    }

    pub fn grammar_version(&self) -> &str {
        &self.grammar_version
    }

    pub fn matches(&self, dialect: &str, grammar_id: &str, grammar_version: &str) -> bool {
        self.dialect == dialect
            && self.grammar_id == grammar_id
            && self.grammar_version == grammar_version
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DialectPolicy {
    support: CapabilitySupport,
    authority: Option<CapabilityAuthority>,
    variants: Vec<DialectDeclaration>,
}

impl DialectPolicy {
    pub fn provided(
        authority: CapabilityAuthority,
        variants: Vec<DialectDeclaration>,
    ) -> Result<Self, String> {
        let policy = Self {
            support: CapabilitySupport::Provided,
            authority: Some(authority),
            variants,
        };
        policy.validate()?;
        Ok(policy)
    }

    pub const fn unsupported() -> Self {
        Self {
            support: CapabilitySupport::Unsupported,
            authority: None,
            variants: Vec::new(),
        }
    }

    pub const fn unknown() -> Self {
        Self {
            support: CapabilitySupport::Unknown,
            authority: None,
            variants: Vec::new(),
        }
    }

    pub fn support(&self) -> CapabilitySupport {
        self.support
    }

    pub fn authority(&self) -> Option<CapabilityAuthority> {
        self.authority
    }

    pub fn variants(&self) -> &[DialectDeclaration] {
        &self.variants
    }

    pub fn declaration(
        &self,
        dialect: &str,
        grammar_id: &str,
        grammar_version: &str,
    ) -> Option<&DialectDeclaration> {
        (self.support == CapabilitySupport::Provided)
            .then(|| {
                self.variants
                    .iter()
                    .find(|variant| variant.matches(dialect, grammar_id, grammar_version))
            })
            .flatten()
    }

    fn validate(&self) -> Result<(), String> {
        match self.support {
            CapabilitySupport::Provided => {
                if self.authority.is_none() || self.variants.is_empty() {
                    return Err("provided dialect policy lacks authority or variants".to_string());
                }
                let mut variants = std::collections::BTreeSet::new();
                for variant in &self.variants {
                    if variant.dialect.trim().is_empty()
                        || variant.grammar_id.trim().is_empty()
                        || variant.grammar_version.trim().is_empty()
                        || !variants.insert((
                            &variant.dialect,
                            &variant.grammar_id,
                            &variant.grammar_version,
                        ))
                    {
                        return Err(
                            "dialect variants have empty or duplicate identities".to_string()
                        );
                    }
                }
            }
            CapabilitySupport::Unsupported | CapabilitySupport::Unknown => {
                if self.authority.is_some() || !self.variants.is_empty() {
                    return Err("unavailable dialect policy retains provided payload".to_string());
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LanguageConstructPolicy {
    schema: String,
    adapter_schema: String,
    parse_recovery: ParseRecoveryPolicy,
    constructs: Vec<ConstructPolicySection>,
    dialects: DialectPolicy,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LanguageConstructPolicyWire {
    schema: String,
    adapter_schema: String,
    parse_recovery: ParseRecoveryPolicy,
    constructs: Vec<ConstructPolicySection>,
    dialects: DialectPolicy,
}

impl<'de> Deserialize<'de> for LanguageConstructPolicy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = LanguageConstructPolicyWire::deserialize(deserializer)?;
        let policy = Self {
            schema: wire.schema,
            adapter_schema: wire.adapter_schema,
            parse_recovery: wire.parse_recovery,
            constructs: wire.constructs,
            dialects: wire.dialects,
        };
        policy.validate().map_err(D::Error::custom)?;
        Ok(policy)
    }
}

impl LanguageConstructPolicy {
    pub fn new(
        adapter_schema: impl Into<String>,
        parse_recovery: ParseRecoveryPolicy,
        constructs: Vec<ConstructPolicySection>,
        dialects: DialectPolicy,
    ) -> Result<Self, String> {
        let policy = Self {
            schema: LANGUAGE_CONSTRUCT_POLICY_SCHEMA.to_string(),
            adapter_schema: adapter_schema.into(),
            parse_recovery,
            constructs,
            dialects,
        };
        policy.validate()?;
        Ok(policy)
    }

    pub fn unknown(adapter_schema: impl Into<String>) -> Self {
        Self::new(
            adapter_schema,
            ParseRecoveryPolicy::unknown(),
            ConstructPolicyKind::ALL
                .into_iter()
                .map(ConstructPolicySection::unknown)
                .collect(),
            DialectPolicy::unknown(),
        )
        .expect("the total unknown construct policy is valid")
    }

    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn adapter_schema(&self) -> &str {
        &self.adapter_schema
    }

    pub fn parse_recovery(&self) -> &ParseRecoveryPolicy {
        &self.parse_recovery
    }

    pub fn constructs(&self) -> &[ConstructPolicySection] {
        &self.constructs
    }

    pub fn construct(&self, kind: ConstructPolicyKind) -> &ConstructPolicySection {
        let index = ConstructPolicyKind::ALL
            .iter()
            .position(|candidate| *candidate == kind)
            .expect("the construct policy catalog is exhaustive");
        &self.constructs[index]
    }

    pub fn dialects(&self) -> &DialectPolicy {
        &self.dialects
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != LANGUAGE_CONSTRUCT_POLICY_SCHEMA {
            return Err(format!(
                "unsupported language construct policy schema {}",
                self.schema
            ));
        }
        if self.adapter_schema.trim().is_empty() {
            return Err("construct policy adapter schema must not be empty".to_string());
        }
        self.parse_recovery.validate()?;
        self.dialects.validate()?;
        if self.constructs.len() != ConstructPolicyKind::ALL.len() {
            return Err("construct policy catalog is incomplete".to_string());
        }
        for (section, expected) in self.constructs.iter().zip(ConstructPolicyKind::ALL) {
            if section.kind != expected {
                return Err(format!(
                    "construct policy catalog is out of order: expected {}, found {}",
                    expected.as_str(),
                    section.kind.as_str()
                ));
            }
            section.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrammarDescriptor {
    lang: Lang,
    dialect: &'static str,
    grammar_id: &'static str,
    grammar_version: &'static str,
}

#[derive(Debug, Clone)]
pub struct ResolvedGrammar {
    descriptor: GrammarDescriptor,
    language: tree_sitter::Language,
}

impl GrammarDescriptor {
    pub const fn new(
        lang: Lang,
        dialect: &'static str,
        grammar_id: &'static str,
        grammar_version: &'static str,
    ) -> Self {
        Self {
            lang,
            dialect,
            grammar_id,
            grammar_version,
        }
    }

    pub fn lang(self) -> Lang {
        self.lang
    }

    pub fn dialect(self) -> &'static str {
        self.dialect
    }

    pub fn grammar_id(self) -> &'static str {
        self.grammar_id
    }

    pub fn grammar_version(self) -> &'static str {
        self.grammar_version
    }
}

impl ResolvedGrammar {
    pub fn into_parts(self) -> (GrammarDescriptor, tree_sitter::Language) {
        (self.descriptor, self.language)
    }
}

pub trait LangPack: Send + Sync {
    fn name(&self) -> &'static str;
    /// Versioned identity for the semantic hooks implemented by this adapter.
    /// Implementations must bump this value whenever hook behavior changes.
    fn adapter_schema(&self) -> &'static str {
        "deslop-lang-adapter/4"
    }
    fn capability_manifest(&self) -> LanguageAdapterCapabilityManifest;
    fn query_pack(&self) -> LanguageQueryPack {
        LanguageQueryPack::unknown(self.adapter_schema())
    }
    fn lexical_policy(&self) -> LanguageLexicalPolicy {
        LanguageLexicalPolicy::unknown(self.adapter_schema())
    }
    fn construct_policy(&self) -> LanguageConstructPolicy {
        LanguageConstructPolicy::unknown(self.adapter_schema())
    }
    fn resolution_rule_pack(&self) -> LanguageResolutionRulePack {
        LanguageResolutionRulePack::unknown(self.adapter_schema())
    }
    fn control_flow_rule_pack(&self) -> LanguageControlFlowRulePack {
        LanguageControlFlowRulePack::unknown(self.adapter_schema())
    }
    fn canonical_roles(&self, _node: Node<'_>, _text: &str) -> CanonicalRoleSet {
        CanonicalRoleSet::default()
    }
    fn lang(&self) -> Lang;
    fn extensions(&self) -> &'static [&'static str];
    fn grammar(&self) -> Option<tree_sitter::Language>;
    fn grammar_for_path(&self, _path: &Path) -> Option<tree_sitter::Language> {
        self.grammar()
    }
    fn grammar_descriptor_for_path(&self, _path: &Path) -> Option<GrammarDescriptor> {
        None
    }
    fn resolve_grammar(&self, path: &Path) -> Option<ResolvedGrammar> {
        Some(ResolvedGrammar {
            descriptor: self.grammar_descriptor_for_path(path)?,
            language: self.grammar_for_path(path)?,
        })
    }
    fn line_comments(&self) -> &'static [&'static str];
    fn metrics_regions(&self) -> &'static [&'static str];
    fn metrics_branches(&self) -> &'static [&'static str];
    fn metrics_nesting(&self) -> &'static [&'static str];
    fn metrics_flow_breaks(&self) -> &'static [&'static str];
    fn metric_branch_contribution(&self, node: Node<'_>, _text: &str) -> usize {
        usize::from(self.metrics_branches().contains(&node.kind()))
    }
    fn is_metric_nesting(&self, node: Node<'_>, _text: &str) -> bool {
        self.metrics_nesting().contains(&node.kind())
    }
    fn is_metric_flow_break(&self, node: Node<'_>, _text: &str) -> bool {
        self.metrics_flow_breaks().contains(&node.kind())
    }
    fn halstead_operator_tokens(&self) -> &'static [&'static str];
    fn region_class(&self, _node: Node<'_>, _text: &str) -> RegionClass {
        RegionClass::Other
    }
    fn is_long_method_region(&self, _node: Node<'_>, _text: &str) -> bool {
        false
    }
    fn is_behavioral_container(&self, _node: Node<'_>, _text: &str) -> bool {
        false
    }
    /// True when `node` is a named-constant definition (e.g. `const`/`static`,
    /// Clojure `def`/`defonce`, Julia `const`). The magic-number rule exempts
    /// numeric literals inside such a definition: binding a literal to a name IS
    /// the rule's recommended fix, so it must not be flagged in turn.
    fn is_constant_definition_region(&self, _node: Node<'_>, _text: &str) -> bool {
        false
    }
    fn is_duplication_data_region(&self, _node: Node<'_>, _text: &str) -> bool {
        false
    }
    fn tail_position_class(&self, _node: Node<'_>, _text: &str) -> TailPositionClass {
        TailPositionClass::Other
    }

    fn enclosing_region(&self, node: Node<'_>, text: &str) -> Option<RegionSpan>;

    fn detect(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| self.extensions().contains(&extension))
    }
}

pub trait Rule<Source, Config, Output>: Send + Sync {
    fn name(&self) -> &'static str;
    fn check(&self, source: &Source, config: &Config) -> Vec<Output>;
}

#[derive(Debug, Clone)]
pub enum ExternalFindings<Output> {
    Available(Vec<Output>),
    Unavailable { notice: String },
}

pub trait ExternalAnalyzer<Source, Output>: Send + Sync {
    fn name(&self) -> &'static str;
    fn covered_rules(&self) -> &'static [&'static str];
    fn analyze(&self, path: &Path, source: &Source) -> Result<ExternalFindings<Output>>;
}

#[derive(Clone)]
pub struct Registry {
    packs: Vec<&'static dyn LangPack>,
    generic: &'static dyn LangPack,
}

impl Registry {
    pub fn new(generic: &'static dyn LangPack) -> Self {
        Self {
            packs: Vec::new(),
            generic,
        }
    }

    pub fn with_default_packs() -> Self {
        let mut registry = Self::new(&GENERIC_PACK);
        registry.register(&CLOJURE_PACK);
        registry.register(&JULIA_PACK);
        registry.register(&PYTHON_PACK);
        registry.register(&JAVASCRIPT_PACK);
        registry.register(&TYPESCRIPT_PACK);
        registry.register(&RUST_PACK);
        registry
    }

    pub fn register(&mut self, pack: &'static dyn LangPack) {
        self.packs.push(pack);
    }

    pub fn pack_for_path(&self, path: &Path) -> &'static dyn LangPack {
        self.packs
            .iter()
            .copied()
            .find(|pack| pack.detect(path))
            .unwrap_or(self.generic)
    }

    pub fn pack_for_lang(&self, lang: Lang) -> &'static dyn LangPack {
        self.packs
            .iter()
            .copied()
            .find(|pack| pack.lang() == lang)
            .unwrap_or(self.generic)
    }

    pub fn supported_pack_for_path(&self, path: &Path) -> Option<&'static dyn LangPack> {
        self.packs.iter().copied().find(|pack| pack.detect(path))
    }

    pub fn resolve_grammar(&self, path: &Path) -> Option<ResolvedGrammar> {
        let pack = self.supported_pack_for_path(path)?;
        let resolved = pack.resolve_grammar(path)?;
        (resolved.descriptor.lang == pack.lang()).then_some(resolved)
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::with_default_packs()
    }
}

pub fn detect_lang(path: &Path) -> Lang {
    Registry::default().pack_for_path(path).lang()
}

pub fn is_supported_source(path: &Path) -> bool {
    Registry::default().supported_pack_for_path(path).is_some()
}

pub static GENERIC_PACK: GenericPack = GenericPack;
pub static CLOJURE_PACK: ClojurePack = ClojurePack;
pub static JULIA_PACK: JuliaPack = JuliaPack;
pub static PYTHON_PACK: PythonPack = PythonPack;
pub static JAVASCRIPT_PACK: JavaScriptPack = JavaScriptPack;
pub static TYPESCRIPT_PACK: TypeScriptPack = TypeScriptPack;
pub static RUST_PACK: RustPack = RustPack;

pub struct GenericPack;
pub struct ClojurePack;
pub struct JuliaPack;
pub struct PythonPack;
pub struct JavaScriptPack;
pub struct TypeScriptPack;
pub struct RustPack;

impl LangPack for GenericPack {
    fn name(&self) -> &'static str {
        "generic"
    }

    fn capability_manifest(&self) -> LanguageAdapterCapabilityManifest {
        LanguageAdapterCapabilityManifest::unknown(self.adapter_schema())
    }

    fn lang(&self) -> Lang {
        Lang::Generic
    }

    fn extensions(&self) -> &'static [&'static str] {
        &[]
    }

    fn grammar(&self) -> Option<tree_sitter::Language> {
        None
    }

    fn grammar_descriptor_for_path(&self, _path: &Path) -> Option<GrammarDescriptor> {
        None
    }

    fn line_comments(&self) -> &'static [&'static str] {
        &["#"]
    }

    fn metrics_regions(&self) -> &'static [&'static str] {
        &[]
    }

    fn metrics_branches(&self) -> &'static [&'static str] {
        &[]
    }

    fn metrics_nesting(&self) -> &'static [&'static str] {
        &[]
    }

    fn metrics_flow_breaks(&self) -> &'static [&'static str] {
        &[]
    }

    fn halstead_operator_tokens(&self) -> &'static [&'static str] {
        &[
            "=", "+", "-", "*", "/", "%", "==", "!=", "<", ">", "<=", ">=", "&&", "||", "!",
        ]
    }

    fn enclosing_region(&self, _node: Node<'_>, _text: &str) -> Option<RegionSpan> {
        None
    }
}

fn clojure_node_is_evaluated(node: Node<'_>) -> bool {
    let mut pending_unquotes = 0usize;
    let mut ancestor = node.parent();
    while let Some(parent) = ancestor {
        match parent.kind() {
            "dis_expr" | "quoting_lit" | "var_quoting_lit" | "evaling_lit" => return false,
            "unquoting_lit" | "unquote_splicing_lit" => pending_unquotes += 1,
            "syn_quoting_lit" if pending_unquotes == 0 => return false,
            "syn_quoting_lit" => pending_unquotes -= 1,
            _ => {}
        }
        ancestor = parent.parent();
    }
    true
}

fn clojure_list_head_token<'a>(node: Node<'_>, text: &'a str) -> Option<&'a str> {
    if node.kind() != "list_lit" {
        return None;
    }
    let head = node.child_by_field_name("value")?;
    if head.kind() != "sym_lit" {
        return None;
    }
    let name = head.child_by_field_name("name")?;
    text.get(name.start_byte()..name.end_byte())
}

fn clojure_canonical_roles(node: Node<'_>, text: &str) -> CanonicalRoleSet {
    let roles = match node.kind() {
        "source" => vec![CanonicalRole::Project, CanonicalRole::Module],
        "list_lit" if clojure_node_is_evaluated(node) => {
            match clojure_list_head_token(node, text) {
                Some("ns") => vec![CanonicalRole::Declaration, CanonicalRole::Module],
                Some("def" | "defonce") => vec![CanonicalRole::Declaration],
                Some("defn" | "defn-" | "defmacro" | "defmethod") => {
                    vec![CanonicalRole::Declaration, CanonicalRole::Callable]
                }
                Some("defrecord" | "deftype" | "defprotocol" | "definterface" | "defmulti") => {
                    vec![CanonicalRole::Declaration, CanonicalRole::Type]
                }
                Some("fn" | "fn*") => vec![CanonicalRole::Callable, CanonicalRole::Block],
                Some("let" | "let*" | "letfn" | "binding") => vec![CanonicalRole::Block],
                Some(
                    "if" | "if-not" | "if-let" | "if-some" | "when" | "when-not" | "when-let"
                    | "when-some" | "cond" | "condp",
                ) => vec![CanonicalRole::Branch],
                Some("case") => vec![CanonicalRole::Branch, CanonicalRole::Match],
                Some("for" | "doseq" | "dotimes" | "while" | "loop") => {
                    vec![CanonicalRole::Loop, CanonicalRole::Block]
                }
                Some("throw" | "recur" | "return") => vec![CanonicalRole::Statement],
                Some(_) | None => vec![CanonicalRole::Expression, CanonicalRole::Call],
            }
        }
        "vec_lit"
            if node.parent().is_some_and(|parent| {
                parent.kind() == "list_lit"
                    && clojure_node_is_evaluated(parent)
                    && matches!(
                        clojure_list_head_token(parent, text),
                        Some("defn" | "defn-" | "fn" | "fn*")
                    )
            }) =>
        {
            vec![CanonicalRole::Parameter]
        }
        "vec_lit" | "map_lit" | "set_lit" | "ns_map_lit" => vec![CanonicalRole::Expression],
        "sym_lit" | "sym_name" | "sym_ns" => {
            vec![CanonicalRole::Expression, CanonicalRole::Read]
        }
        "bool_lit" | "char_lit" | "kwd_lit" | "nil_lit" | "num_lit" | "regex_lit" | "str_lit" => {
            vec![CanonicalRole::Expression, CanonicalRole::Literal]
        }
        "comment"
            if matches!(
                text.get(node.start_byte()..node.end_byte()),
                Some(";; @generated\n" | ";; @generated")
            ) =>
        {
            vec![CanonicalRole::Comment, CanonicalRole::Generated]
        }
        "comment" => vec![CanonicalRole::Comment],
        "ERROR" => vec![CanonicalRole::Error],
        "anon_fn_lit"
        | "derefing_lit"
        | "dis_expr"
        | "evaling_lit"
        | "quoting_lit"
        | "read_cond_lit"
        | "splicing_read_cond_lit"
        | "syn_quoting_lit"
        | "tagged_or_ctor_lit"
        | "unquote_splicing_lit"
        | "unquoting_lit"
        | "var_quoting_lit" => vec![CanonicalRole::OpaqueRegion],
        "meta_lit" if text.get(node.start_byte()..node.end_byte()) == Some("^:generated") => {
            vec![CanonicalRole::Generated]
        }
        _ => Vec::new(),
    };
    CanonicalRoleSet::from_roles(roles)
}

fn clojure_query_pack(adapter_schema: &str) -> LanguageQueryPack {
    let capture = |name, roles: &[CanonicalRole]| {
        QueryCaptureDeclaration::new(name, CanonicalRoleSet::from_roles(roles.iter().copied()))
            .expect("the Clojure query capture is valid")
    };
    LanguageQueryPack::new(
        adapter_schema,
        vec![
            QueryFamilyDeclaration::unknown(QueryFamily::Declarations),
            QueryFamilyDeclaration::unknown(QueryFamily::References),
            QueryFamilyDeclaration::provided(
                QueryFamily::Scopes,
                CapabilityAuthority::Adapter,
                "(source) @scope.module",
                vec![capture("scope.module", &[CanonicalRole::Module])],
            ),
            QueryFamilyDeclaration::unknown(QueryFamily::Control),
            QueryFamilyDeclaration::provided(
                QueryFamily::Comments,
                CapabilityAuthority::Adapter,
                "(comment) @comment",
                vec![capture("comment", &[CanonicalRole::Comment])],
            ),
            QueryFamilyDeclaration::provided(
                QueryFamily::OpaqueGenerated,
                CapabilityAuthority::Adapter,
                "[(anon_fn_lit) (derefing_lit) (dis_expr) (evaling_lit) (quoting_lit) (read_cond_lit) (splicing_read_cond_lit) (syn_quoting_lit) (tagged_or_ctor_lit) (unquote_splicing_lit) (unquoting_lit) (var_quoting_lit)] @opaque",
                vec![capture("opaque", &[CanonicalRole::OpaqueRegion])],
            ),
            // Stored Tree-sitter queries cannot exclude arbitrary quoted
            // ancestors, so contract facts stay unknown like the
            // declaration/reference/control families.
            QueryFamilyDeclaration::unknown(QueryFamily::Contract),
        ],
    )
    .expect("the Clojure query pack is valid")
}

fn clojure_lexical_policy(adapter_schema: &str) -> LanguageLexicalPolicy {
    let token =
        |raw_kind, class| LexicalRule::new(raw_kind, None, LexicalClassification::token(class));
    let symbol_operator = |text: &'static str, class: LexicalOperatorClass| {
        LexicalRule::new(
            "sym_name",
            Some(text.to_string()),
            LexicalClassification::operator(class),
        )
    };
    let mut rules = vec![
        symbol_operator("+", LexicalOperatorClass::Arithmetic),
        symbol_operator("-", LexicalOperatorClass::Arithmetic),
        symbol_operator("*", LexicalOperatorClass::Arithmetic),
        symbol_operator("/", LexicalOperatorClass::Arithmetic),
        symbol_operator("quot", LexicalOperatorClass::Arithmetic),
        symbol_operator("mod", LexicalOperatorClass::Arithmetic),
        symbol_operator("rem", LexicalOperatorClass::Arithmetic),
        symbol_operator("=", LexicalOperatorClass::Comparison),
        symbol_operator("not=", LexicalOperatorClass::Comparison),
        symbol_operator("<", LexicalOperatorClass::Comparison),
        symbol_operator(">", LexicalOperatorClass::Comparison),
        symbol_operator("<=", LexicalOperatorClass::Comparison),
        symbol_operator(">=", LexicalOperatorClass::Comparison),
        symbol_operator("and", LexicalOperatorClass::Logical),
        symbol_operator("or", LexicalOperatorClass::Logical),
        symbol_operator("not", LexicalOperatorClass::Logical),
        symbol_operator("bit-and", LexicalOperatorClass::Bitwise),
        symbol_operator("bit-or", LexicalOperatorClass::Bitwise),
        symbol_operator("bit-xor", LexicalOperatorClass::Bitwise),
        symbol_operator("bit-not", LexicalOperatorClass::Bitwise),
        symbol_operator("bit-shift-left", LexicalOperatorClass::Bitwise),
        symbol_operator("bit-shift-right", LexicalOperatorClass::Bitwise),
    ];
    rules.extend(
        ["sym_name", "sym_ns"]
            .into_iter()
            .map(|kind| token(kind, LexicalTokenClass::Identifier)),
    );
    rules.extend(
        [
            "bool_lit",
            "char_lit",
            "kwd_lit",
            "nil_lit",
            "num_lit",
            "regex_lit",
            "str_lit",
        ]
        .into_iter()
        .map(|kind| token(kind, LexicalTokenClass::Literal)),
    );
    rules.extend(
        ["(", ")", "[", "]", "{", "}"]
            .into_iter()
            .map(|kind| token(kind, LexicalTokenClass::Delimiter)),
    );
    rules.extend(
        [
            "#", "##", "#'", "#=", "#?", "#?@", "#^", "#_", "'", "`", "~", "~@", "^", "@", ":",
            "::",
        ]
        .into_iter()
        .map(|kind| token(kind, LexicalTokenClass::Punctuation)),
    );
    rules.extend([
        token("comment", LexicalTokenClass::Comment),
        token("ERROR", LexicalTokenClass::Error),
        token("*", LexicalTokenClass::Other),
    ]);
    LanguageLexicalPolicy::provided(
        adapter_schema,
        CapabilityAuthority::Adapter,
        IdentifierCasePolicy::Sensitive,
        true,
        vec![";".to_string()],
        Vec::new(),
        rules,
    )
    .expect("the Clojure lexical policy is valid")
}

fn clojure_construct_policy(adapter_schema: &str) -> LanguageConstructPolicy {
    LanguageConstructPolicy::new(
        adapter_schema,
        ParseRecoveryPolicy::provided(
            CapabilityAuthority::Syntax,
            ParseRecoveryHandling::FileIncomplete,
        ),
        vec![
            ConstructPolicySection::provided(
                ConstructPolicyKind::UnsupportedConstruct,
                CapabilityAuthority::Adapter,
                vec![ConstructRule::new(
                    "evaling_lit",
                    None,
                    ConstructHandling::Opaque,
                )],
            )
            .expect("the Clojure unsupported policy is valid"),
            ConstructPolicySection::provided(
                ConstructPolicyKind::Macro,
                CapabilityAuthority::Adapter,
                vec![
                    ConstructRule::new("anon_fn_lit", None, ConstructHandling::Opaque),
                    ConstructRule::new("derefing_lit", None, ConstructHandling::Opaque),
                    ConstructRule::new("dis_expr", None, ConstructHandling::Opaque),
                    ConstructRule::new("quoting_lit", None, ConstructHandling::Opaque),
                    ConstructRule::new("read_cond_lit", None, ConstructHandling::Opaque),
                    ConstructRule::new("splicing_read_cond_lit", None, ConstructHandling::Opaque),
                    ConstructRule::new("syn_quoting_lit", None, ConstructHandling::Opaque),
                    ConstructRule::new("tagged_or_ctor_lit", None, ConstructHandling::Opaque),
                    ConstructRule::new("unquote_splicing_lit", None, ConstructHandling::Opaque),
                    ConstructRule::new("unquoting_lit", None, ConstructHandling::Opaque),
                    ConstructRule::new("var_quoting_lit", None, ConstructHandling::Opaque),
                ],
            )
            .expect("the Clojure macro policy is valid"),
            ConstructPolicySection::provided(
                ConstructPolicyKind::GeneratedCode,
                CapabilityAuthority::Adapter,
                vec![
                    ConstructRule::new(
                        "comment",
                        Some(";; @generated\n".to_string()),
                        ConstructHandling::SurfaceSyntax,
                    ),
                    ConstructRule::new(
                        "comment",
                        Some(";; @generated".to_string()),
                        ConstructHandling::SurfaceSyntax,
                    ),
                    ConstructRule::new(
                        "meta_lit",
                        Some("^:generated".to_string()),
                        ConstructHandling::SurfaceSyntax,
                    ),
                ],
            )
            .expect("the Clojure generated policy is valid"),
        ],
        DialectPolicy::provided(
            CapabilityAuthority::Syntax,
            vec![DialectDeclaration::new(
                "clojure",
                "tree-sitter-clojure",
                "0.1.0",
            )],
        )
        .expect("the Clojure dialect policy is valid"),
    )
    .expect("the Clojure construct policy is valid")
}

impl LangPack for ClojurePack {
    fn name(&self) -> &'static str {
        "clojure"
    }

    fn capability_manifest(&self) -> LanguageAdapterCapabilityManifest {
        LanguageAdapterCapabilityManifest::current_syntax(self.adapter_schema())
            .with_declaration(CapabilityDeclaration::provided(
                AdapterCapability::CanonicalRoles,
                CapabilityAuthority::Adapter,
            ))
            .expect("the Clojure S1 capability declaration is valid")
    }

    fn canonical_roles(&self, node: Node<'_>, text: &str) -> CanonicalRoleSet {
        clojure_canonical_roles(node, text)
    }

    fn query_pack(&self) -> LanguageQueryPack {
        clojure_query_pack(self.adapter_schema())
    }

    fn lexical_policy(&self) -> LanguageLexicalPolicy {
        clojure_lexical_policy(self.adapter_schema())
    }

    fn construct_policy(&self) -> LanguageConstructPolicy {
        clojure_construct_policy(self.adapter_schema())
    }

    fn resolution_rule_pack(&self) -> LanguageResolutionRulePack {
        crate::resolution::builtin_resolution_rule_pack(
            self.adapter_schema(),
            crate::resolution::BuiltinResolutionFamily::Clojure,
        )
    }

    fn lang(&self) -> Lang {
        Lang::Clojure
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["clj", "cljs", "cljc", "edn"]
    }

    fn grammar(&self) -> Option<tree_sitter::Language> {
        Some(tree_sitter_clojure::LANGUAGE.into())
    }

    fn grammar_descriptor_for_path(&self, _path: &Path) -> Option<GrammarDescriptor> {
        Some(GrammarDescriptor {
            lang: Lang::Clojure,
            dialect: "clojure",
            grammar_id: "tree-sitter-clojure",
            grammar_version: "0.1.0",
        })
    }

    fn line_comments(&self) -> &'static [&'static str] {
        &[";"]
    }

    fn metrics_regions(&self) -> &'static [&'static str] {
        &["list_lit"]
    }

    fn metrics_branches(&self) -> &'static [&'static str] {
        &[
            "if",
            "if-not",
            "if-let",
            "if-some",
            "when",
            "when-not",
            "when-let",
            "when-some",
            "cond",
            "condp",
            "case",
            "for",
            "doseq",
            "dotimes",
            "while",
            "loop",
        ]
    }

    fn metrics_nesting(&self) -> &'static [&'static str] {
        self.metrics_branches()
    }

    fn metrics_flow_breaks(&self) -> &'static [&'static str] {
        &["throw", "recur"]
    }

    fn halstead_operator_tokens(&self) -> &'static [&'static str] {
        &[
            "defn",
            "defmacro",
            "defmethod",
            "fn",
            "let",
            "if",
            "if-not",
            "if-let",
            "if-some",
            "when",
            "when-not",
            "when-let",
            "when-some",
            "cond",
            "condp",
            "case",
            "for",
            "doseq",
            "dotimes",
            "while",
            "loop",
            "recur",
            "=",
            "not=",
            "+",
            "-",
            "*",
            "/",
            ">",
            "<",
            ">=",
            "<=",
        ]
    }

    fn metric_branch_contribution(&self, node: Node<'_>, text: &str) -> usize {
        usize::from(
            clojure_form_is_evaluated(node)
                && matches!(
                    clojure_list_head_token(node, text),
                    Some(
                        "if" | "if-not"
                            | "if-let"
                            | "if-some"
                            | "when"
                            | "when-not"
                            | "when-let"
                            | "when-some"
                            | "cond"
                            | "condp"
                            | "case"
                            | "for"
                            | "doseq"
                            | "dotimes"
                            | "while"
                            | "loop"
                    )
                ),
        )
    }

    fn is_metric_nesting(&self, node: Node<'_>, text: &str) -> bool {
        self.metric_branch_contribution(node, text) > 0
    }

    fn is_metric_flow_break(&self, node: Node<'_>, text: &str) -> bool {
        clojure_form_is_evaluated(node)
            && matches!(clojure_list_head_token(node, text), Some("throw" | "recur"))
    }

    fn region_class(&self, node: Node<'_>, text: &str) -> RegionClass {
        if node.kind() != "list_lit" {
            return RegionClass::Other;
        }
        match clojure_list_head_token(node, text) {
            Some("defn" | "defmacro" | "defmethod" | "fn") => RegionClass::Behavioral,
            Some(
                "ns" | "require" | "import" | "def" | "defrecord" | "deftype" | "defprotocol"
                | "definterface" | "defmulti",
            ) => RegionClass::Declaration,
            _ => RegionClass::Other,
        }
    }

    fn is_long_method_region(&self, node: Node<'_>, text: &str) -> bool {
        self.region_class(node, text) == RegionClass::Behavioral
    }

    fn is_constant_definition_region(&self, node: Node<'_>, text: &str) -> bool {
        node.kind() == "list_lit"
            && matches!(clojure_list_head_token(node, text), Some("def" | "defonce"))
    }

    fn is_duplication_data_region(&self, node: Node<'_>, _text: &str) -> bool {
        matches!(node.kind(), "map_lit" | "set_lit")
    }

    fn enclosing_region(&self, node: Node<'_>, text: &str) -> Option<RegionSpan> {
        top_level_clojure_list(node, text)
    }
}

fn clojure_form_is_evaluated(node: Node<'_>) -> bool {
    node.kind() == "list_lit" && clojure_node_is_evaluated(node)
}

fn julia_canonical_roles(node: Node<'_>, text: &str) -> CanonicalRoleSet {
    let roles = match node.kind() {
        "source_file" => vec![CanonicalRole::Project, CanonicalRole::Module],
        "function_definition" => vec![
            CanonicalRole::Declaration,
            CanonicalRole::Callable,
            CanonicalRole::Block,
        ],
        "arrow_function_expression" | "do_clause" => vec![CanonicalRole::Callable],
        "struct_definition" | "abstract_definition" | "primitive_definition" => {
            vec![CanonicalRole::Declaration, CanonicalRole::Type]
        }
        "module_definition" => vec![CanonicalRole::Declaration, CanonicalRole::Module],
        "macro_definition" => vec![
            CanonicalRole::Declaration,
            CanonicalRole::Callable,
            CanonicalRole::OpaqueRegion,
        ],
        "import_statement" | "using_statement" => {
            vec![CanonicalRole::Declaration, CanonicalRole::Import]
        }
        "export_statement" | "public_statement" => vec![CanonicalRole::Export],
        "const_statement" => vec![CanonicalRole::Declaration],
        "argument_list" | "macro_argument_list" if julia_is_signature_arguments(node) => {
            vec![CanonicalRole::Parameter]
        }
        "compound_statement" | "let_statement" => vec![CanonicalRole::Block],
        "return_statement" | "break_statement" | "continue_statement" | "global_statement"
        | "local_statement" => vec![CanonicalRole::Statement],
        "if_statement" | "elseif_clause" | "if_clause" | "try_statement" => {
            vec![CanonicalRole::Branch]
        }
        "catch_clause" | "else_clause" | "finally_clause" => vec![CanonicalRole::Case],
        "for_statement" | "for_clause" | "while_statement" => vec![CanonicalRole::Loop],
        "call_expression" | "broadcast_call_expression" => {
            vec![CanonicalRole::Expression, CanonicalRole::Call]
        }
        "assignment" | "compound_assignment_expression" => {
            vec![CanonicalRole::Expression, CanonicalRole::Write]
        }
        "identifier" | "scoped_identifier" | "field_expression" | "macro_identifier" => {
            vec![CanonicalRole::Expression, CanonicalRole::Read]
        }
        "boolean_literal"
        | "character_literal"
        | "command_literal"
        | "float_literal"
        | "integer_literal"
        | "prefixed_command_literal"
        | "prefixed_string_literal"
        | "string_literal" => vec![CanonicalRole::Expression, CanonicalRole::Literal],
        "vector_expression"
        | "matrix_expression"
        | "tuple_expression"
        | "comprehension_expression"
        | "range_expression" => vec![CanonicalRole::Expression],
        "line_comment" if text.get(node.start_byte()..node.end_byte()) == Some("# @generated") => {
            vec![CanonicalRole::Comment, CanonicalRole::Generated]
        }
        "line_comment" | "block_comment" => vec![CanonicalRole::Comment],
        "ERROR" => vec![CanonicalRole::Error],
        "quote_expression" | "quote_statement" => vec![CanonicalRole::OpaqueRegion],
        "macrocall_expression"
            if text.get(node.start_byte()..node.end_byte()) == Some("@generated") =>
        {
            vec![CanonicalRole::Generated, CanonicalRole::OpaqueRegion]
        }
        "macrocall_expression" => vec![CanonicalRole::OpaqueRegion],
        _ => Vec::new(),
    };
    CanonicalRoleSet::from_roles(roles)
}

fn julia_is_signature_arguments(node: Node<'_>) -> bool {
    let mut ancestor = node.parent();
    while let Some(parent) = ancestor {
        match parent.kind() {
            "signature" | "macro_definition" => return true,
            "compound_statement" | "function_definition" | "do_clause" => return false,
            _ => ancestor = parent.parent(),
        }
    }
    false
}

fn julia_query_pack(adapter_schema: &str) -> LanguageQueryPack {
    let capture = |name, roles: &[CanonicalRole]| {
        QueryCaptureDeclaration::new(name, CanonicalRoleSet::from_roles(roles.iter().copied()))
            .expect("the Julia query capture is valid")
    };
    LanguageQueryPack::new(
        adapter_schema,
        vec![
            QueryFamilyDeclaration::provided(
                QueryFamily::Declarations,
                CapabilityAuthority::Adapter,
                "[(function_definition) (struct_definition) (abstract_definition) (primitive_definition) (module_definition) (macro_definition) (import_statement) (using_statement) (const_statement)] @declaration",
                vec![capture("declaration", &[CanonicalRole::Declaration])],
            ),
            QueryFamilyDeclaration::provided(
                QueryFamily::References,
                CapabilityAuthority::Adapter,
                "(call_expression . (_) @reference)",
                vec![capture(
                    "reference",
                    &[CanonicalRole::Expression, CanonicalRole::Read],
                )],
            ),
            QueryFamilyDeclaration::provided(
                QueryFamily::Scopes,
                CapabilityAuthority::Adapter,
                "(source_file) @scope.module\n[(function_definition) (let_statement)] @scope.block",
                vec![
                    capture("scope.module", &[CanonicalRole::Module]),
                    capture("scope.block", &[CanonicalRole::Block]),
                ],
            ),
            QueryFamilyDeclaration::provided(
                QueryFamily::Control,
                CapabilityAuthority::Adapter,
                "[(if_statement) (elseif_clause) (if_clause) (try_statement)] @control.branch\n[(for_statement) (for_clause) (while_statement)] @control.loop",
                vec![
                    capture("control.branch", &[CanonicalRole::Branch]),
                    capture("control.loop", &[CanonicalRole::Loop]),
                ],
            ),
            QueryFamilyDeclaration::provided(
                QueryFamily::Comments,
                CapabilityAuthority::Adapter,
                "[(line_comment) (block_comment)] @comment",
                vec![capture("comment", &[CanonicalRole::Comment])],
            ),
            QueryFamilyDeclaration::provided(
                QueryFamily::OpaqueGenerated,
                CapabilityAuthority::Adapter,
                "[(macro_definition) (macrocall_expression) (quote_expression) (quote_statement)] @opaque",
                vec![capture("opaque", &[CanonicalRole::OpaqueRegion])],
            ),
            QueryFamilyDeclaration::provided(
                QueryFamily::Contract,
                CapabilityAuthority::Adapter,
                "(assignment . (call_expression (identifier) @function.name)) @function.assign\n(function_definition (call_expression (identifier) @function.name)) @function\n(function_definition (signature (call_expression (identifier) @function.name))) @function\n(call_expression (identifier) @ref)\n(index_expression (identifier) @config.object (vector_expression (string_literal) @config.key))\n(for_statement) @loop\n(while_statement) @loop\n(macrocall_expression (macro_identifier) @assert.macro)\n(call_expression) @call.expr",
                vec![
                    capture("function.name", &[CanonicalRole::Read]),
                    capture("function.assign", &[CanonicalRole::Write]),
                    capture("function", &[CanonicalRole::Callable]),
                    capture("ref", &[CanonicalRole::Read]),
                    capture("config.object", &[CanonicalRole::Read]),
                    capture("config.key", &[CanonicalRole::Literal]),
                    capture("loop", &[CanonicalRole::Loop]),
                    capture("assert.macro", &[CanonicalRole::Read]),
                    capture("call.expr", &[CanonicalRole::Call]),
                ],
            ),
        ],
    )
    .expect("the Julia query pack is valid")
}

fn julia_lexical_policy(adapter_schema: &str) -> LanguageLexicalPolicy {
    let token =
        |raw_kind, class| LexicalRule::new(raw_kind, None, LexicalClassification::token(class));
    let operator = |text: &'static str, class: LexicalOperatorClass| {
        LexicalRule::new(
            "operator",
            Some(text.to_string()),
            LexicalClassification::operator(class),
        )
    };
    let mut rules = vec![
        operator("+", LexicalOperatorClass::Arithmetic),
        operator("-", LexicalOperatorClass::Arithmetic),
        operator("*", LexicalOperatorClass::Arithmetic),
        operator("/", LexicalOperatorClass::Arithmetic),
        operator("÷", LexicalOperatorClass::Arithmetic),
        operator("%", LexicalOperatorClass::Arithmetic),
        operator("^", LexicalOperatorClass::Arithmetic),
        operator("==", LexicalOperatorClass::Comparison),
        operator("!=", LexicalOperatorClass::Comparison),
        operator("===", LexicalOperatorClass::Comparison),
        operator("!==", LexicalOperatorClass::Comparison),
        operator("<", LexicalOperatorClass::Comparison),
        operator(">", LexicalOperatorClass::Comparison),
        operator("<=", LexicalOperatorClass::Comparison),
        operator(">=", LexicalOperatorClass::Comparison),
        operator("&&", LexicalOperatorClass::Logical),
        operator("||", LexicalOperatorClass::Logical),
        operator("!", LexicalOperatorClass::Logical),
        operator("&", LexicalOperatorClass::Bitwise),
        operator("|", LexicalOperatorClass::Bitwise),
        operator("⊻", LexicalOperatorClass::Bitwise),
        operator("~", LexicalOperatorClass::Bitwise),
        operator("<<", LexicalOperatorClass::Bitwise),
        operator(">>", LexicalOperatorClass::Bitwise),
        operator(">>>", LexicalOperatorClass::Bitwise),
        operator(":", LexicalOperatorClass::Range),
        operator(".", LexicalOperatorClass::MemberAccess),
        operator("=", LexicalOperatorClass::Assignment),
        operator("+=", LexicalOperatorClass::Assignment),
        operator("-=", LexicalOperatorClass::Assignment),
        operator("*=", LexicalOperatorClass::Assignment),
        operator("/=", LexicalOperatorClass::Assignment),
        operator("\\=", LexicalOperatorClass::Assignment),
        operator("÷=", LexicalOperatorClass::Assignment),
        operator("%=", LexicalOperatorClass::Assignment),
        operator("^=", LexicalOperatorClass::Assignment),
        operator("&=", LexicalOperatorClass::Assignment),
        operator("|=", LexicalOperatorClass::Assignment),
        operator("⊻=", LexicalOperatorClass::Assignment),
        operator("<<=", LexicalOperatorClass::Assignment),
        operator(">>=", LexicalOperatorClass::Assignment),
        operator(">>>=", LexicalOperatorClass::Assignment),
    ];
    rules.extend([
        LexicalRule::new(
            "=",
            None,
            LexicalClassification::operator(LexicalOperatorClass::Assignment),
        ),
        LexicalRule::new(
            ".=",
            None,
            LexicalClassification::operator(LexicalOperatorClass::Assignment),
        ),
        LexicalRule::new(
            ":=",
            None,
            LexicalClassification::operator(LexicalOperatorClass::Assignment),
        ),
    ]);
    rules.extend(
        ["identifier", "macro_identifier"]
            .into_iter()
            .map(|kind| token(kind, LexicalTokenClass::Identifier)),
    );
    rules.extend(
        [
            "boolean_literal",
            "character_literal",
            "command_literal",
            "content",
            "escape_sequence",
            "float_literal",
            "integer_literal",
            "true",
            "false",
        ]
        .into_iter()
        .map(|kind| token(kind, LexicalTokenClass::Literal)),
    );
    rules.extend(
        [
            "abstract",
            "baremodule",
            "begin",
            "catch",
            "const",
            "do",
            "else",
            "elseif",
            "end",
            "export",
            "finally",
            "for",
            "function",
            "global",
            "if",
            "import",
            "let",
            "local",
            "macro",
            "module",
            "mutable",
            "outer",
            "primitive",
            "public",
            "quote",
            "return",
            "struct",
            "try",
            "using",
            "where",
            "while",
        ]
        .into_iter()
        .map(|kind| token(kind, LexicalTokenClass::Keyword)),
    );
    rules.extend(
        ["(", ")", "[", "]", "{", "}"]
            .into_iter()
            .map(|kind| token(kind, LexicalTokenClass::Delimiter)),
    );
    rules.extend(
        [
            "\"", "\"\"\"", "'", "`", "```", "$", ",", ";", "::", "->", "...", "@", "?",
        ]
        .into_iter()
        .map(|kind| token(kind, LexicalTokenClass::Punctuation)),
    );
    rules.extend([
        token("line_comment", LexicalTokenClass::Comment),
        token("block_comment", LexicalTokenClass::Comment),
        token("operator", LexicalTokenClass::Other),
        token("ERROR", LexicalTokenClass::Error),
        token("*", LexicalTokenClass::Other),
    ]);
    LanguageLexicalPolicy::provided(
        adapter_schema,
        CapabilityAuthority::Adapter,
        IdentifierCasePolicy::Sensitive,
        true,
        vec!["#".to_string()],
        vec![BlockCommentDelimiter::new("#=", "=#", true)],
        rules,
    )
    .expect("the Julia lexical policy is valid")
}

fn julia_construct_policy(adapter_schema: &str) -> LanguageConstructPolicy {
    LanguageConstructPolicy::new(
        adapter_schema,
        ParseRecoveryPolicy::provided(
            CapabilityAuthority::Syntax,
            ParseRecoveryHandling::FileIncomplete,
        ),
        vec![
            ConstructPolicySection::provided(
                ConstructPolicyKind::UnsupportedConstruct,
                CapabilityAuthority::Adapter,
                vec![
                    ConstructRule::new("quote_expression", None, ConstructHandling::Opaque),
                    ConstructRule::new("quote_statement", None, ConstructHandling::Opaque),
                ],
            )
            .expect("the Julia unsupported policy is valid"),
            ConstructPolicySection::provided(
                ConstructPolicyKind::Macro,
                CapabilityAuthority::Adapter,
                vec![
                    ConstructRule::new("macro_definition", None, ConstructHandling::Opaque),
                    ConstructRule::new("macrocall_expression", None, ConstructHandling::Opaque),
                ],
            )
            .expect("the Julia macro policy is valid"),
            ConstructPolicySection::provided(
                ConstructPolicyKind::GeneratedCode,
                CapabilityAuthority::Adapter,
                vec![
                    ConstructRule::new(
                        "line_comment",
                        Some("# @generated".to_string()),
                        ConstructHandling::SurfaceSyntax,
                    ),
                    ConstructRule::new(
                        "macrocall_expression",
                        Some("@generated".to_string()),
                        ConstructHandling::SurfaceSyntax,
                    ),
                ],
            )
            .expect("the Julia generated policy is valid"),
        ],
        DialectPolicy::provided(
            CapabilityAuthority::Syntax,
            vec![DialectDeclaration::new(
                "julia",
                "tree-sitter-julia",
                "0.23.1",
            )],
        )
        .expect("the Julia dialect policy is valid"),
    )
    .expect("the Julia construct policy is valid")
}

impl LangPack for JuliaPack {
    fn name(&self) -> &'static str {
        "julia"
    }

    fn capability_manifest(&self) -> LanguageAdapterCapabilityManifest {
        LanguageAdapterCapabilityManifest::current_syntax(self.adapter_schema())
            .with_declaration(CapabilityDeclaration::provided(
                AdapterCapability::CanonicalRoles,
                CapabilityAuthority::Adapter,
            ))
            .expect("the Julia S1 capability declaration is valid")
    }

    fn canonical_roles(&self, node: Node<'_>, text: &str) -> CanonicalRoleSet {
        julia_canonical_roles(node, text)
    }

    fn query_pack(&self) -> LanguageQueryPack {
        julia_query_pack(self.adapter_schema())
    }

    fn lexical_policy(&self) -> LanguageLexicalPolicy {
        julia_lexical_policy(self.adapter_schema())
    }

    fn construct_policy(&self) -> LanguageConstructPolicy {
        julia_construct_policy(self.adapter_schema())
    }

    fn resolution_rule_pack(&self) -> LanguageResolutionRulePack {
        crate::resolution::builtin_resolution_rule_pack(
            self.adapter_schema(),
            crate::resolution::BuiltinResolutionFamily::Julia,
        )
    }

    fn lang(&self) -> Lang {
        Lang::Julia
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["jl"]
    }

    fn grammar(&self) -> Option<tree_sitter::Language> {
        Some(tree_sitter_julia::LANGUAGE.into())
    }

    fn grammar_descriptor_for_path(&self, _path: &Path) -> Option<GrammarDescriptor> {
        Some(GrammarDescriptor {
            lang: Lang::Julia,
            dialect: "julia",
            grammar_id: "tree-sitter-julia",
            grammar_version: "0.23.1",
        })
    }

    fn line_comments(&self) -> &'static [&'static str] {
        &["#"]
    }

    fn metrics_regions(&self) -> &'static [&'static str] {
        &[
            "function_definition",
            "struct_definition",
            "module_definition",
        ]
    }

    fn metrics_branches(&self) -> &'static [&'static str] {
        &[
            "if_statement",
            "elseif_clause",
            "for_statement",
            "while_statement",
            "try_statement",
            "catch_clause",
        ]
    }

    fn metrics_nesting(&self) -> &'static [&'static str] {
        &[
            "if_statement",
            "for_statement",
            "while_statement",
            "try_statement",
            "function_definition",
        ]
    }

    fn metrics_flow_breaks(&self) -> &'static [&'static str] {
        &["return_statement", "break_statement", "continue_statement"]
    }

    fn halstead_operator_tokens(&self) -> &'static [&'static str] {
        &[
            "=", "+", "-", "*", "/", "%", "==", "!=", "<", ">", "<=", ">=", "&&", "||", "!", "if",
            "elseif", "else", "for", "while", "return", "break", "continue",
        ]
    }

    fn region_class(&self, node: Node<'_>, text: &str) -> RegionClass {
        match node.kind() {
            "function_definition" | "do_clause" => RegionClass::Behavioral,
            "struct_definition" => RegionClass::Declaration,
            _ => match node_head_token(node, text) {
                Some("using" | "import" | "const") => RegionClass::Declaration,
                _ => RegionClass::Other,
            },
        }
    }

    fn is_long_method_region(&self, node: Node<'_>, text: &str) -> bool {
        self.region_class(node, text) == RegionClass::Behavioral
    }

    fn is_constant_definition_region(&self, node: Node<'_>, text: &str) -> bool {
        node.kind() == "const_statement" || node_head_token(node, text) == Some("const")
    }

    fn is_duplication_data_region(&self, node: Node<'_>, _text: &str) -> bool {
        matches!(
            node.kind(),
            "vect_expression" | "matrix_expression" | "tuple_expression"
        )
    }

    fn enclosing_region(&self, node: Node<'_>, text: &str) -> Option<RegionSpan> {
        enclosing_julia_block(node, text)
    }
}

fn python_canonical_roles(node: Node<'_>, text: &str) -> CanonicalRoleSet {
    let roles = match node.kind() {
        "module" => vec![CanonicalRole::Project, CanonicalRole::Module],
        "function_definition" => vec![CanonicalRole::Declaration, CanonicalRole::Callable],
        "class_definition" => vec![CanonicalRole::Declaration, CanonicalRole::Type],
        "decorated_definition" => match python_definition_kind(node) {
            Some("function_definition") => {
                vec![CanonicalRole::Declaration, CanonicalRole::Callable]
            }
            Some("class_definition") => vec![CanonicalRole::Declaration, CanonicalRole::Type],
            _ => Vec::new(),
        },
        "lambda" => vec![CanonicalRole::Callable],
        "import_statement" | "import_from_statement" | "future_import_statement" => {
            vec![CanonicalRole::Declaration, CanonicalRole::Import]
        }
        "type_alias_statement" => vec![CanonicalRole::Declaration, CanonicalRole::Type],
        "parameters"
        | "lambda_parameters"
        | "default_parameter"
        | "typed_parameter"
        | "typed_default_parameter"
        | "list_splat_pattern"
        | "dictionary_splat_pattern" => vec![CanonicalRole::Parameter],
        "block" => vec![CanonicalRole::Block],
        "expression_statement"
        | "assert_statement"
        | "delete_statement"
        | "global_statement"
        | "nonlocal_statement"
        | "pass_statement"
        | "raise_statement"
        | "return_statement"
        | "break_statement"
        | "continue_statement" => vec![CanonicalRole::Statement],
        "if_statement" | "elif_clause" => vec![CanonicalRole::Branch],
        "match_statement" => vec![CanonicalRole::Branch, CanonicalRole::Match],
        "case_clause" => vec![CanonicalRole::Case],
        "for_statement" | "while_statement" | "for_in_clause" => vec![CanonicalRole::Loop],
        "call" => vec![CanonicalRole::Expression, CanonicalRole::Call],
        "assignment" | "augmented_assignment" | "named_expression" => {
            vec![CanonicalRole::Expression, CanonicalRole::Write]
        }
        "identifier" | "attribute" => vec![CanonicalRole::Expression, CanonicalRole::Read],
        "integer" | "float" | "string" | "true" | "false" | "none" | "ellipsis" => {
            vec![CanonicalRole::Expression, CanonicalRole::Literal]
        }
        "list"
        | "set"
        | "tuple"
        | "dictionary"
        | "list_comprehension"
        | "set_comprehension"
        | "dictionary_comprehension"
        | "generator_expression" => vec![CanonicalRole::Expression],
        "comment" => vec![CanonicalRole::Comment],
        "ERROR" => vec![CanonicalRole::Error],
        "exec_statement" | "print_statement" => vec![CanonicalRole::OpaqueRegion],
        "decorator" if text.get(node.start_byte()..node.end_byte()) == Some("@generated") => {
            vec![CanonicalRole::Generated]
        }
        _ => Vec::new(),
    };
    CanonicalRoleSet::from_roles(roles)
}

fn python_query_pack(adapter_schema: &str) -> LanguageQueryPack {
    let capture = |name, roles: &[CanonicalRole]| {
        QueryCaptureDeclaration::new(name, CanonicalRoleSet::from_roles(roles.iter().copied()))
            .expect("the Python query capture is valid")
    };
    LanguageQueryPack::new(
        adapter_schema,
        vec![
            QueryFamilyDeclaration::provided(
                QueryFamily::Declarations,
                CapabilityAuthority::Adapter,
                "[(function_definition) (class_definition) (decorated_definition) (import_statement) (import_from_statement) (future_import_statement) (type_alias_statement)] @declaration",
                vec![capture("declaration", &[CanonicalRole::Declaration])],
            ),
            QueryFamilyDeclaration::provided(
                QueryFamily::References,
                CapabilityAuthority::Adapter,
                "(call function: (_) @reference)",
                vec![capture(
                    "reference",
                    &[CanonicalRole::Expression, CanonicalRole::Read],
                )],
            ),
            QueryFamilyDeclaration::provided(
                QueryFamily::Scopes,
                CapabilityAuthority::Adapter,
                "(module) @scope.module\n(block) @scope.block",
                vec![
                    capture("scope.module", &[CanonicalRole::Module]),
                    capture("scope.block", &[CanonicalRole::Block]),
                ],
            ),
            QueryFamilyDeclaration::provided(
                QueryFamily::Control,
                CapabilityAuthority::Adapter,
                "[(if_statement) (elif_clause) (match_statement)] @control.branch\n[(for_statement) (while_statement)] @control.loop",
                vec![
                    capture("control.branch", &[CanonicalRole::Branch]),
                    capture("control.loop", &[CanonicalRole::Loop]),
                ],
            ),
            QueryFamilyDeclaration::provided(
                QueryFamily::Comments,
                CapabilityAuthority::Adapter,
                "(comment) @comment",
                vec![capture("comment", &[CanonicalRole::Comment])],
            ),
            QueryFamilyDeclaration::provided(
                QueryFamily::OpaqueGenerated,
                CapabilityAuthority::Adapter,
                "[(exec_statement) (print_statement)] @opaque",
                vec![capture("opaque", &[CanonicalRole::OpaqueRegion])],
            ),
            QueryFamilyDeclaration::provided(
                QueryFamily::Contract,
                CapabilityAuthority::Adapter,
                "(function_definition name: (identifier) @function.name) @function\n(call function: [(identifier) (attribute)] @ref)\n(string) @string\n(subscript value: [(identifier) (attribute)] @config.object subscript: (string) @config.key)\n(call function: (attribute) @config.accessor arguments: (argument_list . (string) @config.key))\n(for_statement) @loop\n(while_statement) @loop\n(for_in_clause) @loop\n(assert_statement) @assertion\n(raise_statement) @assertion\n(call) @call.expr",
                vec![
                    capture("function.name", &[CanonicalRole::Read]),
                    capture("function", &[CanonicalRole::Callable]),
                    capture("ref", &[CanonicalRole::Read]),
                    capture("string", &[CanonicalRole::Literal]),
                    capture("config.object", &[CanonicalRole::Read]),
                    capture("config.key", &[CanonicalRole::Literal]),
                    capture("config.accessor", &[CanonicalRole::Read]),
                    capture("loop", &[CanonicalRole::Loop]),
                    capture("assertion", &[CanonicalRole::Statement]),
                    capture("call.expr", &[CanonicalRole::Call]),
                ],
            ),
        ],
    )
    .expect("the Python query pack is valid")
}

fn python_lexical_policy(adapter_schema: &str) -> LanguageLexicalPolicy {
    let token =
        |raw_kind, class| LexicalRule::new(raw_kind, None, LexicalClassification::token(class));
    let exact_token = |raw_kind: &'static str, text: &'static str, class| {
        LexicalRule::new(
            raw_kind,
            Some(text.to_string()),
            LexicalClassification::token(class),
        )
    };
    let operator = |raw_kind: &'static str, text: Option<&str>, class: LexicalOperatorClass| {
        LexicalRule::new(
            raw_kind,
            text.map(str::to_string),
            LexicalClassification::operator(class),
        )
    };
    let mut rules = vec![
        operator("+", None, LexicalOperatorClass::Arithmetic),
        operator("-", None, LexicalOperatorClass::Arithmetic),
        operator("*", Some("*"), LexicalOperatorClass::Arithmetic),
        operator("/", None, LexicalOperatorClass::Arithmetic),
        operator("//", None, LexicalOperatorClass::Arithmetic),
        operator("%", None, LexicalOperatorClass::Arithmetic),
        operator("**", None, LexicalOperatorClass::Arithmetic),
        operator("@", None, LexicalOperatorClass::Arithmetic),
        operator("==", None, LexicalOperatorClass::Comparison),
        operator("!=", None, LexicalOperatorClass::Comparison),
        operator("<", None, LexicalOperatorClass::Comparison),
        operator(">", None, LexicalOperatorClass::Comparison),
        operator("<=", None, LexicalOperatorClass::Comparison),
        operator(">=", None, LexicalOperatorClass::Comparison),
        operator("is", None, LexicalOperatorClass::Comparison),
        operator("in", None, LexicalOperatorClass::Comparison),
        operator("and", None, LexicalOperatorClass::Logical),
        operator("or", None, LexicalOperatorClass::Logical),
        operator("not", None, LexicalOperatorClass::Logical),
        operator("=", None, LexicalOperatorClass::Assignment),
        operator("+=", None, LexicalOperatorClass::Assignment),
        operator("-=", None, LexicalOperatorClass::Assignment),
        operator("*=", None, LexicalOperatorClass::Assignment),
        operator("/=", None, LexicalOperatorClass::Assignment),
        operator("//=", None, LexicalOperatorClass::Assignment),
        operator("%=", None, LexicalOperatorClass::Assignment),
        operator("**=", None, LexicalOperatorClass::Assignment),
        operator("@=", None, LexicalOperatorClass::Assignment),
        operator("&=", None, LexicalOperatorClass::Assignment),
        operator("|=", None, LexicalOperatorClass::Assignment),
        operator("^=", None, LexicalOperatorClass::Assignment),
        operator(">>=", None, LexicalOperatorClass::Assignment),
        operator("<<=", None, LexicalOperatorClass::Assignment),
        operator("&", None, LexicalOperatorClass::Bitwise),
        operator("|", None, LexicalOperatorClass::Bitwise),
        operator("^", None, LexicalOperatorClass::Bitwise),
        operator("~", None, LexicalOperatorClass::Bitwise),
        operator("<<", None, LexicalOperatorClass::Bitwise),
        operator(">>", None, LexicalOperatorClass::Bitwise),
        operator(".", None, LexicalOperatorClass::MemberAccess),
        operator(":=", None, LexicalOperatorClass::Other),
    ];
    rules.extend(
        ["identifier"]
            .into_iter()
            .map(|kind| token(kind, LexicalTokenClass::Identifier)),
    );
    rules.extend(
        [
            "integer",
            "float",
            "string",
            "string_content",
            "escape_interpolation",
            "true",
            "false",
            "none",
            "ellipsis",
        ]
        .into_iter()
        .map(|kind| token(kind, LexicalTokenClass::Literal)),
    );
    rules.extend(
        [
            "def", "class", "return", "raise", "if", "elif", "else", "match", "case", "for",
            "while", "break", "continue", "try", "except", "finally", "with", "as", "import",
            "from", "pass", "async", "global", "nonlocal", "assert", "del",
        ]
        .into_iter()
        .map(|kind| token(kind, LexicalTokenClass::Keyword)),
    );
    rules.extend(
        ["await", "lambda", "type", "yield"]
            .into_iter()
            .map(|kind| exact_token(kind, kind, LexicalTokenClass::Keyword)),
    );
    rules.extend(
        ["(", ")", "[", "]", "{", "}"]
            .into_iter()
            .map(|kind| token(kind, LexicalTokenClass::Delimiter)),
    );
    rules.extend(
        [";", ",", ":", "->"]
            .into_iter()
            .map(|kind| token(kind, LexicalTokenClass::Punctuation)),
    );
    rules.extend([
        token("comment", LexicalTokenClass::Comment),
        token("ERROR", LexicalTokenClass::Error),
        token("*", LexicalTokenClass::Other),
    ]);
    LanguageLexicalPolicy::provided(
        adapter_schema,
        CapabilityAuthority::Adapter,
        IdentifierCasePolicy::Sensitive,
        true,
        vec!["#".to_string()],
        Vec::new(),
        rules,
    )
    .expect("the Python lexical policy is valid")
}

fn python_construct_policy(adapter_schema: &str) -> LanguageConstructPolicy {
    LanguageConstructPolicy::new(
        adapter_schema,
        ParseRecoveryPolicy::provided(
            CapabilityAuthority::Syntax,
            ParseRecoveryHandling::FileIncomplete,
        ),
        vec![
            ConstructPolicySection::provided(
                ConstructPolicyKind::UnsupportedConstruct,
                CapabilityAuthority::Adapter,
                vec![
                    ConstructRule::new("exec_statement", None, ConstructHandling::Opaque),
                    ConstructRule::new("print_statement", None, ConstructHandling::Opaque),
                ],
            )
            .expect("the Python unsupported policy is valid"),
            ConstructPolicySection::unsupported(ConstructPolicyKind::Macro),
            ConstructPolicySection::provided(
                ConstructPolicyKind::GeneratedCode,
                CapabilityAuthority::Adapter,
                vec![
                    ConstructRule::new(
                        "comment",
                        Some("# @generated".to_string()),
                        ConstructHandling::SurfaceSyntax,
                    ),
                    ConstructRule::new(
                        "decorator",
                        Some("@generated".to_string()),
                        ConstructHandling::SurfaceSyntax,
                    ),
                ],
            )
            .expect("the Python generated policy is valid"),
        ],
        DialectPolicy::provided(
            CapabilityAuthority::Syntax,
            vec![DialectDeclaration::new(
                "python",
                "tree-sitter-python",
                "0.25.0",
            )],
        )
        .expect("the Python dialect policy is valid"),
    )
    .expect("the Python construct policy is valid")
}

impl LangPack for PythonPack {
    fn name(&self) -> &'static str {
        "python"
    }

    fn capability_manifest(&self) -> LanguageAdapterCapabilityManifest {
        LanguageAdapterCapabilityManifest::current_syntax(self.adapter_schema())
            .with_declaration(CapabilityDeclaration::provided(
                AdapterCapability::CanonicalRoles,
                CapabilityAuthority::Adapter,
            ))
            .expect("the Python S1 capability declaration is valid")
    }

    fn canonical_roles(&self, node: Node<'_>, text: &str) -> CanonicalRoleSet {
        python_canonical_roles(node, text)
    }

    fn query_pack(&self) -> LanguageQueryPack {
        python_query_pack(self.adapter_schema())
    }

    fn lexical_policy(&self) -> LanguageLexicalPolicy {
        python_lexical_policy(self.adapter_schema())
    }

    fn construct_policy(&self) -> LanguageConstructPolicy {
        python_construct_policy(self.adapter_schema())
    }

    fn resolution_rule_pack(&self) -> LanguageResolutionRulePack {
        crate::resolution::builtin_resolution_rule_pack(
            self.adapter_schema(),
            crate::resolution::BuiltinResolutionFamily::Python,
        )
    }

    fn lang(&self) -> Lang {
        Lang::Python
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["py"]
    }

    fn grammar(&self) -> Option<tree_sitter::Language> {
        Some(tree_sitter_python::LANGUAGE.into())
    }

    fn grammar_descriptor_for_path(&self, _path: &Path) -> Option<GrammarDescriptor> {
        Some(GrammarDescriptor {
            lang: Lang::Python,
            dialect: "python",
            grammar_id: "tree-sitter-python",
            grammar_version: "0.25.0",
        })
    }

    fn line_comments(&self) -> &'static [&'static str] {
        &["#"]
    }

    fn metrics_regions(&self) -> &'static [&'static str] {
        &["function_definition", "class_definition"]
    }

    fn metrics_branches(&self) -> &'static [&'static str] {
        &[
            "if_statement",
            "elif_clause",
            "for_statement",
            "while_statement",
            "except_clause",
        ]
    }

    fn metrics_nesting(&self) -> &'static [&'static str] {
        &[
            "if_statement",
            "for_statement",
            "while_statement",
            "try_statement",
        ]
    }

    fn metrics_flow_breaks(&self) -> &'static [&'static str] {
        &[
            "return_statement",
            "break_statement",
            "continue_statement",
            "raise_statement",
        ]
    }

    fn halstead_operator_tokens(&self) -> &'static [&'static str] {
        &[
            "=", "+", "-", "*", "/", "%", "==", "!=", "<", ">", "<=", ">=", "and", "or", "not",
            "if", "elif", "else", "for", "while", "return", "raise",
        ]
    }

    fn region_class(&self, node: Node<'_>, _text: &str) -> RegionClass {
        match python_definition_kind(node) {
            Some("function_definition") => RegionClass::Behavioral,
            Some("class_definition") => RegionClass::Declaration,
            _ => RegionClass::Other,
        }
    }

    fn is_long_method_region(&self, node: Node<'_>, _text: &str) -> bool {
        match node.kind() {
            "decorated_definition" => python_definition_kind(node) == Some("function_definition"),
            "function_definition" => node
                .parent()
                .is_none_or(|parent| parent.kind() != "decorated_definition"),
            _ => false,
        }
    }

    fn is_behavioral_container(&self, node: Node<'_>, _text: &str) -> bool {
        python_definition_kind(node) == Some("class_definition")
    }

    fn enclosing_region(&self, mut node: Node<'_>, text: &str) -> Option<RegionSpan> {
        loop {
            if matches!(node.kind(), "function_definition" | "class_definition") {
                if let Some(parent) = node.parent()
                    && parent.kind() == "decorated_definition"
                {
                    return Some(region_from_node(parent, text));
                }
                return Some(region_from_node(node, text));
            }
            if node.kind() == "decorated_definition" {
                return Some(region_from_node(node, text));
            }
            node = node.parent()?;
        }
    }
}

fn python_definition_kind(node: Node<'_>) -> Option<&str> {
    match node.kind() {
        "function_definition" | "class_definition" => Some(node.kind()),
        "decorated_definition" => node
            .child_by_field_name("definition")
            .map(|node| node.kind()),
        _ => None,
    }
}

fn ecma_canonical_roles(node: Node<'_>, text: &str) -> CanonicalRoleSet {
    let roles = match node.kind() {
        "program" => vec![CanonicalRole::Project, CanonicalRole::Module],
        "function_declaration" | "generator_function_declaration" | "method_definition" => {
            vec![CanonicalRole::Declaration, CanonicalRole::Callable]
        }
        "function_expression" | "generator_function" | "arrow_function" => {
            vec![CanonicalRole::Callable]
        }
        "class_declaration"
        | "abstract_class_declaration"
        | "interface_declaration"
        | "type_alias_declaration"
        | "enum_declaration" => {
            vec![CanonicalRole::Declaration, CanonicalRole::Type]
        }
        "import_statement" => vec![CanonicalRole::Declaration, CanonicalRole::Import],
        "export_statement" => vec![CanonicalRole::Export],
        "formal_parameters" | "required_parameter" | "optional_parameter" => {
            vec![CanonicalRole::Parameter]
        }
        "statement_block" => vec![CanonicalRole::Block],
        "lexical_declaration"
        | "variable_declaration"
        | "expression_statement"
        | "throw_statement" => {
            vec![CanonicalRole::Statement]
        }
        "if_statement" | "switch_statement" => vec![CanonicalRole::Branch],
        "switch_case" | "switch_default" => vec![CanonicalRole::Case],
        "for_statement" | "for_in_statement" | "while_statement" | "do_statement" => {
            vec![CanonicalRole::Loop]
        }
        "call_expression" | "new_expression" => {
            vec![CanonicalRole::Expression, CanonicalRole::Call]
        }
        "assignment_expression" | "augmented_assignment_expression" => {
            vec![CanonicalRole::Expression, CanonicalRole::Write]
        }
        "identifier" | "property_identifier" | "private_property_identifier" => {
            vec![CanonicalRole::Expression, CanonicalRole::Read]
        }
        "member_expression"
            if node.parent().is_some_and(|parent| {
                parent.kind() == "call_expression"
                    && parent
                        .child_by_field_name("function")
                        .is_some_and(|function| function.id() == node.id())
            }) =>
        {
            vec![CanonicalRole::Expression, CanonicalRole::Read]
        }
        "string" | "number" | "true" | "false" | "null" | "undefined" | "template_string"
        | "regex" => vec![CanonicalRole::Expression, CanonicalRole::Literal],
        "jsx_element" | "jsx_self_closing_element" | "jsx_fragment" => {
            vec![CanonicalRole::Expression]
        }
        "comment" => vec![CanonicalRole::Comment],
        "ERROR" => vec![CanonicalRole::Error],
        "with_statement" => vec![CanonicalRole::OpaqueRegion],
        "decorator" if text.get(node.start_byte()..node.end_byte()) == Some("@generated") => {
            vec![CanonicalRole::Generated]
        }
        _ => Vec::new(),
    };
    CanonicalRoleSet::from_roles(roles)
}

fn ecma_query_pack(adapter_schema: &str, typed: bool) -> LanguageQueryPack {
    let capture = |name, roles: &[CanonicalRole]| {
        QueryCaptureDeclaration::new(name, CanonicalRoleSet::from_roles(roles.iter().copied()))
            .expect("the ECMAScript query capture is valid")
    };
    let declarations = if typed {
        "[(function_declaration) (generator_function_declaration) (class_declaration) (abstract_class_declaration) (method_definition) (import_statement) (interface_declaration) (type_alias_declaration) (enum_declaration)] @declaration"
    } else {
        "[(function_declaration) (generator_function_declaration) (class_declaration) (method_definition) (import_statement)] @declaration"
    };
    LanguageQueryPack::new(
        adapter_schema,
        vec![
            QueryFamilyDeclaration::provided(
                QueryFamily::Declarations,
                CapabilityAuthority::Adapter,
                declarations,
                vec![capture("declaration", &[CanonicalRole::Declaration])],
            ),
            QueryFamilyDeclaration::provided(
                QueryFamily::References,
                CapabilityAuthority::Adapter,
                "(call_expression function: (_) @reference)",
                vec![capture(
                    "reference",
                    &[CanonicalRole::Expression, CanonicalRole::Read],
                )],
            ),
            QueryFamilyDeclaration::provided(
                QueryFamily::Scopes,
                CapabilityAuthority::Adapter,
                "(program) @scope.module\n(statement_block) @scope.block",
                vec![
                    capture("scope.module", &[CanonicalRole::Module]),
                    capture("scope.block", &[CanonicalRole::Block]),
                ],
            ),
            QueryFamilyDeclaration::provided(
                QueryFamily::Control,
                CapabilityAuthority::Adapter,
                "[(if_statement) (switch_statement)] @control.branch\n[(for_statement) (for_in_statement) (while_statement) (do_statement)] @control.loop",
                vec![
                    capture("control.branch", &[CanonicalRole::Branch]),
                    capture("control.loop", &[CanonicalRole::Loop]),
                ],
            ),
            QueryFamilyDeclaration::provided(
                QueryFamily::Comments,
                CapabilityAuthority::Adapter,
                "(comment) @comment",
                vec![capture("comment", &[CanonicalRole::Comment])],
            ),
            QueryFamilyDeclaration::provided(
                QueryFamily::OpaqueGenerated,
                CapabilityAuthority::Adapter,
                "(with_statement) @opaque",
                vec![capture("opaque", &[CanonicalRole::OpaqueRegion])],
            ),
            // Contract facts are provided for plain JavaScript; the typed
            // grammars stay an honest unknown gap until their queries are
            // validated against the TypeScript/TSX grammars.
            if typed {
                QueryFamilyDeclaration::unknown(QueryFamily::Contract)
            } else {
                QueryFamilyDeclaration::provided(
                    QueryFamily::Contract,
                    CapabilityAuthority::Adapter,
                    "(function_declaration name: (identifier) @function.name) @function\n(method_definition name: (property_identifier) @function.name) @function\n(variable_declarator name: (identifier) @function.name value: [(arrow_function) (function_expression)] @function.value)\n(call_expression function: [(identifier) (member_expression)] @ref)\n(string) @string\n(member_expression object: (member_expression property: (property_identifier) @config.object) property: (property_identifier) @config.prop)\n(subscript_expression object: (member_expression property: (property_identifier) @config.object) index: (string) @config.key)\n(for_statement) @loop\n(for_in_statement) @loop\n(while_statement) @loop\n(throw_statement) @assertion\n(call_expression) @call.expr",
                    vec![
                        capture("function.name", &[CanonicalRole::Read]),
                        capture("function", &[CanonicalRole::Callable]),
                        capture("function.value", &[CanonicalRole::Callable]),
                        capture("ref", &[CanonicalRole::Read]),
                        capture("string", &[CanonicalRole::Literal]),
                        capture("config.object", &[CanonicalRole::Read]),
                        capture("config.prop", &[CanonicalRole::Read]),
                        capture("config.key", &[CanonicalRole::Literal]),
                        capture("loop", &[CanonicalRole::Loop]),
                        capture("assertion", &[CanonicalRole::Statement]),
                        capture("call.expr", &[CanonicalRole::Call]),
                    ],
                )
            },
        ],
    )
    .expect("the ECMAScript query pack is valid")
}

fn ecma_lexical_policy(adapter_schema: &str) -> LanguageLexicalPolicy {
    let token =
        |raw_kind, class| LexicalRule::new(raw_kind, None, LexicalClassification::token(class));
    let operator = |raw_kind: &'static str, text: Option<&str>, class: LexicalOperatorClass| {
        LexicalRule::new(
            raw_kind,
            text.map(str::to_string),
            LexicalClassification::operator(class),
        )
    };
    let mut rules = vec![
        operator("+", None, LexicalOperatorClass::Arithmetic),
        operator("-", None, LexicalOperatorClass::Arithmetic),
        operator("*", Some("*"), LexicalOperatorClass::Arithmetic),
        operator("/", None, LexicalOperatorClass::Arithmetic),
        operator("%", None, LexicalOperatorClass::Arithmetic),
        operator("**", None, LexicalOperatorClass::Arithmetic),
        operator("++", None, LexicalOperatorClass::Arithmetic),
        operator("--", None, LexicalOperatorClass::Arithmetic),
        operator("==", None, LexicalOperatorClass::Comparison),
        operator("===", None, LexicalOperatorClass::Comparison),
        operator("!=", None, LexicalOperatorClass::Comparison),
        operator("!==", None, LexicalOperatorClass::Comparison),
        operator("<", None, LexicalOperatorClass::Comparison),
        operator(">", None, LexicalOperatorClass::Comparison),
        operator("<=", None, LexicalOperatorClass::Comparison),
        operator(">=", None, LexicalOperatorClass::Comparison),
        operator("instanceof", None, LexicalOperatorClass::Comparison),
        operator("in", None, LexicalOperatorClass::Comparison),
        operator("&&", None, LexicalOperatorClass::Logical),
        operator("||", None, LexicalOperatorClass::Logical),
        operator("!", None, LexicalOperatorClass::Logical),
        operator("??", None, LexicalOperatorClass::Logical),
        operator("=", None, LexicalOperatorClass::Assignment),
        operator("+=", None, LexicalOperatorClass::Assignment),
        operator("-=", None, LexicalOperatorClass::Assignment),
        operator("*=", None, LexicalOperatorClass::Assignment),
        operator("/=", None, LexicalOperatorClass::Assignment),
        operator("%=", None, LexicalOperatorClass::Assignment),
        operator("**=", None, LexicalOperatorClass::Assignment),
        operator("&&=", None, LexicalOperatorClass::Assignment),
        operator("||=", None, LexicalOperatorClass::Assignment),
        operator("??=", None, LexicalOperatorClass::Assignment),
        operator("&", None, LexicalOperatorClass::Bitwise),
        operator("|", None, LexicalOperatorClass::Bitwise),
        operator("^", None, LexicalOperatorClass::Bitwise),
        operator("~", None, LexicalOperatorClass::Bitwise),
        operator("<<", None, LexicalOperatorClass::Bitwise),
        operator(">>", None, LexicalOperatorClass::Bitwise),
        operator(">>>", None, LexicalOperatorClass::Bitwise),
        operator(".", None, LexicalOperatorClass::MemberAccess),
        operator("?.", None, LexicalOperatorClass::MemberAccess),
        operator("typeof", None, LexicalOperatorClass::Other),
        operator("delete", None, LexicalOperatorClass::Other),
        operator("void", None, LexicalOperatorClass::Other),
        operator("new", None, LexicalOperatorClass::Other),
    ];
    rules.extend(
        [
            "identifier",
            "property_identifier",
            "private_property_identifier",
            "shorthand_property_identifier_pattern",
            "type_identifier",
        ]
        .into_iter()
        .map(|kind| token(kind, LexicalTokenClass::Identifier)),
    );
    rules.extend(
        [
            "number",
            "string",
            "regex",
            "true",
            "false",
            "null",
            "undefined",
            "string_fragment",
            "escape_sequence",
            "jsx_text",
        ]
        .into_iter()
        .map(|kind| token(kind, LexicalTokenClass::Literal)),
    );
    rules.extend(
        [
            "const",
            "let",
            "var",
            "function",
            "class",
            "extends",
            "return",
            "throw",
            "if",
            "else",
            "switch",
            "case",
            "default",
            "for",
            "while",
            "do",
            "break",
            "continue",
            "try",
            "catch",
            "finally",
            "import",
            "export",
            "from",
            "as",
            "async",
            "await",
            "yield",
            "of",
            "this",
            "super",
            "static",
            "get",
            "set",
            "interface",
            "type",
            "enum",
            "implements",
            "declare",
            "namespace",
            "abstract",
            "readonly",
            "keyof",
            "infer",
            "satisfies",
            "public",
            "private",
            "protected",
        ]
        .into_iter()
        .map(|kind| token(kind, LexicalTokenClass::Keyword)),
    );
    rules.extend(
        ["(", ")", "[", "]", "{", "}"]
            .into_iter()
            .map(|kind| token(kind, LexicalTokenClass::Delimiter)),
    );
    rules.extend(
        [";", ",", ":", "?", "=>", "...", "@", "#", "`", "${"]
            .into_iter()
            .map(|kind| token(kind, LexicalTokenClass::Punctuation)),
    );
    rules.extend([
        token("comment", LexicalTokenClass::Comment),
        token("ERROR", LexicalTokenClass::Error),
        token("*", LexicalTokenClass::Other),
    ]);
    LanguageLexicalPolicy::provided(
        adapter_schema,
        CapabilityAuthority::Adapter,
        IdentifierCasePolicy::Sensitive,
        true,
        vec!["//".to_string()],
        vec![BlockCommentDelimiter::new("/*", "*/", false)],
        rules,
    )
    .expect("the ECMAScript lexical policy is valid")
}

fn ecma_construct_policy(
    adapter_schema: &str,
    dialects: Vec<DialectDeclaration>,
) -> LanguageConstructPolicy {
    LanguageConstructPolicy::new(
        adapter_schema,
        ParseRecoveryPolicy::provided(
            CapabilityAuthority::Syntax,
            ParseRecoveryHandling::FileIncomplete,
        ),
        vec![
            ConstructPolicySection::provided(
                ConstructPolicyKind::UnsupportedConstruct,
                CapabilityAuthority::Adapter,
                vec![ConstructRule::new(
                    "with_statement",
                    None,
                    ConstructHandling::Opaque,
                )],
            )
            .expect("the ECMAScript unsupported policy is valid"),
            ConstructPolicySection::unsupported(ConstructPolicyKind::Macro),
            ConstructPolicySection::provided(
                ConstructPolicyKind::GeneratedCode,
                CapabilityAuthority::Adapter,
                vec![
                    ConstructRule::new(
                        "comment",
                        Some("/* @generated */".to_string()),
                        ConstructHandling::SurfaceSyntax,
                    ),
                    ConstructRule::new(
                        "decorator",
                        Some("@generated".to_string()),
                        ConstructHandling::SurfaceSyntax,
                    ),
                ],
            )
            .expect("the ECMAScript generated policy is valid"),
        ],
        DialectPolicy::provided(CapabilityAuthority::Syntax, dialects)
            .expect("the ECMAScript dialect policy is valid"),
    )
    .expect("the ECMAScript construct policy is valid")
}

impl LangPack for JavaScriptPack {
    fn name(&self) -> &'static str {
        "javascript"
    }

    fn capability_manifest(&self) -> LanguageAdapterCapabilityManifest {
        LanguageAdapterCapabilityManifest::current_syntax(self.adapter_schema())
            .with_declaration(CapabilityDeclaration::provided(
                AdapterCapability::CanonicalRoles,
                CapabilityAuthority::Adapter,
            ))
            .expect("the JavaScript S1 capability declaration is valid")
    }

    fn canonical_roles(&self, node: Node<'_>, text: &str) -> CanonicalRoleSet {
        ecma_canonical_roles(node, text)
    }

    fn query_pack(&self) -> LanguageQueryPack {
        ecma_query_pack(self.adapter_schema(), false)
    }

    fn lexical_policy(&self) -> LanguageLexicalPolicy {
        ecma_lexical_policy(self.adapter_schema())
    }

    fn construct_policy(&self) -> LanguageConstructPolicy {
        ecma_construct_policy(
            self.adapter_schema(),
            vec![
                DialectDeclaration::new("javascript", "tree-sitter-javascript", "0.25.0"),
                DialectDeclaration::new("jsx", "tree-sitter-javascript", "0.25.0"),
            ],
        )
    }

    fn resolution_rule_pack(&self) -> LanguageResolutionRulePack {
        crate::resolution::builtin_resolution_rule_pack(
            self.adapter_schema(),
            crate::resolution::BuiltinResolutionFamily::JavaScript,
        )
    }

    fn lang(&self) -> Lang {
        Lang::JavaScript
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["js", "jsx"]
    }

    fn grammar(&self) -> Option<tree_sitter::Language> {
        Some(tree_sitter_javascript::LANGUAGE.into())
    }

    fn grammar_descriptor_for_path(&self, path: &Path) -> Option<GrammarDescriptor> {
        Some(GrammarDescriptor {
            lang: Lang::JavaScript,
            dialect: if path.extension().and_then(|extension| extension.to_str()) == Some("jsx") {
                "jsx"
            } else {
                "javascript"
            },
            grammar_id: "tree-sitter-javascript",
            grammar_version: "0.25.0",
        })
    }

    fn line_comments(&self) -> &'static [&'static str] {
        &["//"]
    }

    fn metrics_regions(&self) -> &'static [&'static str] {
        &[
            "function_declaration",
            "function",
            "arrow_function",
            "method_definition",
            "class_declaration",
        ]
    }

    fn metrics_branches(&self) -> &'static [&'static str] {
        &[
            "if_statement",
            "for_statement",
            "while_statement",
            "try_statement",
            "switch_statement",
        ]
    }

    fn metrics_nesting(&self) -> &'static [&'static str] {
        &[
            "if_statement",
            "for_statement",
            "while_statement",
            "try_statement",
            "function",
            "arrow_function",
            "method_definition",
            "class_declaration",
        ]
    }

    fn metrics_flow_breaks(&self) -> &'static [&'static str] {
        &[
            "return_statement",
            "break_statement",
            "continue_statement",
            "throw_statement",
        ]
    }

    fn halstead_operator_tokens(&self) -> &'static [&'static str] {
        &[
            "=", "+", "-", "*", "/", "%", "===", "!==", "==", "!=", "<", ">", "<=", ">=", "&&",
            "||", "!", "let", "const", "var", "if", "else", "for", "while", "return", "throw",
        ]
    }

    fn region_class(&self, node: Node<'_>, _text: &str) -> RegionClass {
        match node.kind() {
            "function_declaration"
            | "function_expression"
            | "arrow_function"
            | "method_definition" => RegionClass::Behavioral,
            "class_declaration" | "interface_declaration" | "import_statement" => {
                RegionClass::Declaration
            }
            _ => RegionClass::Other,
        }
    }

    fn is_long_method_region(&self, node: Node<'_>, _text: &str) -> bool {
        matches!(
            node.kind(),
            "function_declaration" | "function_expression" | "arrow_function" | "method_definition"
        )
    }

    fn is_constant_definition_region(&self, node: Node<'_>, text: &str) -> bool {
        if !matches!(node.kind(), "lexical_declaration" | "variable_declaration") {
            return false;
        }
        node_head_token(node, text).is_some_and(|head| head == "const")
    }

    fn is_duplication_data_region(&self, node: Node<'_>, _text: &str) -> bool {
        matches!(
            node.kind(),
            "array" | "object" | "object_pattern" | "array_pattern"
        )
    }

    fn enclosing_region(&self, node: Node<'_>, text: &str) -> Option<RegionSpan> {
        let mut node = node;
        loop {
            if matches!(
                node.kind(),
                "function_declaration"
                    | "function_expression"
                    | "arrow_function"
                    | "method_definition"
                    | "class_declaration"
            ) {
                return Some(region_from_node(node, text));
            }
            node = node.parent()?;
        }
    }
}

impl LangPack for TypeScriptPack {
    fn name(&self) -> &'static str {
        "typescript"
    }

    fn capability_manifest(&self) -> LanguageAdapterCapabilityManifest {
        LanguageAdapterCapabilityManifest::current_syntax(self.adapter_schema())
            .with_declaration(CapabilityDeclaration::provided(
                AdapterCapability::CanonicalRoles,
                CapabilityAuthority::Adapter,
            ))
            .expect("the TypeScript S1 capability declaration is valid")
    }

    fn canonical_roles(&self, node: Node<'_>, text: &str) -> CanonicalRoleSet {
        ecma_canonical_roles(node, text)
    }

    fn query_pack(&self) -> LanguageQueryPack {
        ecma_query_pack(self.adapter_schema(), true)
    }

    fn lexical_policy(&self) -> LanguageLexicalPolicy {
        ecma_lexical_policy(self.adapter_schema())
    }

    fn construct_policy(&self) -> LanguageConstructPolicy {
        ecma_construct_policy(
            self.adapter_schema(),
            vec![
                DialectDeclaration::new(
                    "typescript",
                    "tree-sitter-typescript/typescript",
                    "0.23.2",
                ),
                DialectDeclaration::new("tsx", "tree-sitter-typescript/tsx", "0.23.2"),
            ],
        )
    }

    fn resolution_rule_pack(&self) -> LanguageResolutionRulePack {
        crate::resolution::builtin_resolution_rule_pack(
            self.adapter_schema(),
            crate::resolution::BuiltinResolutionFamily::TypeScript,
        )
    }

    fn lang(&self) -> Lang {
        Lang::TypeScript
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["ts", "tsx", "mts", "cts"]
    }

    fn grammar(&self) -> Option<tree_sitter::Language> {
        Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
    }

    fn grammar_for_path(&self, path: &Path) -> Option<tree_sitter::Language> {
        if path.extension().and_then(|extension| extension.to_str()) == Some("tsx") {
            Some(tree_sitter_typescript::LANGUAGE_TSX.into())
        } else {
            self.grammar()
        }
    }

    fn grammar_descriptor_for_path(&self, path: &Path) -> Option<GrammarDescriptor> {
        let tsx = path.extension().and_then(|extension| extension.to_str()) == Some("tsx");
        Some(GrammarDescriptor {
            lang: Lang::TypeScript,
            dialect: if tsx { "tsx" } else { "typescript" },
            grammar_id: if tsx {
                "tree-sitter-typescript/tsx"
            } else {
                "tree-sitter-typescript/typescript"
            },
            grammar_version: "0.23.2",
        })
    }

    fn line_comments(&self) -> &'static [&'static str] {
        &["//"]
    }

    fn metrics_regions(&self) -> &'static [&'static str] {
        &[
            "function_declaration",
            "method_definition",
            "arrow_function",
            "class_declaration",
        ]
    }

    fn metrics_branches(&self) -> &'static [&'static str] {
        &[
            "if_statement",
            "for_statement",
            "while_statement",
            "try_statement",
            "switch_statement",
        ]
    }

    fn metrics_nesting(&self) -> &'static [&'static str] {
        &[
            "if_statement",
            "for_statement",
            "while_statement",
            "try_statement",
            "function_declaration",
            "arrow_function",
            "class_declaration",
            "method_definition",
        ]
    }

    fn metrics_flow_breaks(&self) -> &'static [&'static str] {
        &[
            "return_statement",
            "throw_statement",
            "break_statement",
            "continue_statement",
        ]
    }

    fn halstead_operator_tokens(&self) -> &'static [&'static str] {
        &[
            "=", "+", "-", "*", "/", "%", "===", "!==", "==", "!=", "<", ">", "<=", ">=", "&&",
            "||", "!", "const", "let", "var", "if", "else", "for", "while", "return", "throw",
        ]
    }

    fn region_class(&self, node: Node<'_>, _text: &str) -> RegionClass {
        match node.kind() {
            "function_declaration"
            | "function_expression"
            | "arrow_function"
            | "method_definition" => RegionClass::Behavioral,
            "class_declaration" | "interface_declaration" | "import_statement" => {
                RegionClass::Declaration
            }
            _ => RegionClass::Other,
        }
    }

    fn is_long_method_region(&self, node: Node<'_>, _text: &str) -> bool {
        matches!(
            node.kind(),
            "function_declaration" | "function_expression" | "arrow_function" | "method_definition"
        )
    }

    fn is_constant_definition_region(&self, node: Node<'_>, text: &str) -> bool {
        if !matches!(node.kind(), "lexical_declaration" | "variable_declaration") {
            return false;
        }
        node_head_token(node, text).is_some_and(|head| head == "const")
    }

    fn is_duplication_data_region(&self, node: Node<'_>, _text: &str) -> bool {
        matches!(
            node.kind(),
            "array" | "object" | "object_pattern" | "array_pattern"
        )
    }

    fn enclosing_region(&self, node: Node<'_>, text: &str) -> Option<RegionSpan> {
        let mut node = node;
        loop {
            if matches!(
                node.kind(),
                "function_declaration"
                    | "function_expression"
                    | "arrow_function"
                    | "method_definition"
                    | "class_declaration"
            ) {
                return Some(region_from_node(node, text));
            }
            node = node.parent()?;
        }
    }
}

fn rust_canonical_roles(node: Node<'_>, text: &str) -> CanonicalRoleSet {
    let roles = match node.kind() {
        "source_file" => vec![CanonicalRole::Project, CanonicalRole::Module],
        "function_item" | "function_signature_item" => {
            vec![CanonicalRole::Declaration, CanonicalRole::Callable]
        }
        "struct_item" | "enum_item" | "union_item" | "type_item" | "trait_item" | "impl_item" => {
            vec![CanonicalRole::Declaration, CanonicalRole::Type]
        }
        "mod_item" => vec![CanonicalRole::Declaration, CanonicalRole::Module],
        "const_item" | "static_item" => vec![CanonicalRole::Declaration],
        "macro_definition" => vec![CanonicalRole::Declaration, CanonicalRole::OpaqueRegion],
        "use_declaration" => vec![CanonicalRole::Declaration, CanonicalRole::Import],
        "parameters" | "parameter" | "self_parameter" | "variadic_parameter" => {
            vec![CanonicalRole::Parameter]
        }
        "block" => vec![CanonicalRole::Block],
        "let_declaration" | "expression_statement" => vec![CanonicalRole::Statement],
        "if_expression" => vec![CanonicalRole::Expression, CanonicalRole::Branch],
        "match_expression" => vec![CanonicalRole::Expression, CanonicalRole::Match],
        "match_arm" => vec![CanonicalRole::Case],
        "loop_expression" | "while_expression" | "for_expression" => {
            vec![CanonicalRole::Expression, CanonicalRole::Loop]
        }
        "call_expression" => vec![CanonicalRole::Expression, CanonicalRole::Call],
        "assignment_expression" | "compound_assignment_expr" => {
            vec![CanonicalRole::Expression, CanonicalRole::Write]
        }
        "identifier" | "field_identifier" | "type_identifier" => {
            vec![CanonicalRole::Expression, CanonicalRole::Read]
        }
        "scoped_identifier" | "field_expression"
            if node.parent().is_some_and(|parent| {
                parent.kind() == "call_expression"
                    && parent
                        .child_by_field_name("function")
                        .is_some_and(|function| function.id() == node.id())
            }) =>
        {
            vec![CanonicalRole::Expression, CanonicalRole::Read]
        }
        "integer_literal" | "float_literal" | "char_literal" | "string_literal"
        | "raw_string_literal" | "boolean_literal" => {
            vec![CanonicalRole::Expression, CanonicalRole::Literal]
        }
        "line_comment" | "block_comment" => vec![CanonicalRole::Comment],
        "ERROR" => vec![CanonicalRole::Error],
        "macro_invocation" => vec![CanonicalRole::Expression, CanonicalRole::OpaqueRegion],
        "attribute_item"
            if matches!(
                text.get(node.start_byte()..node.end_byte()),
                Some("#[generated]" | "#[automatically_derived]")
            ) =>
        {
            vec![CanonicalRole::Generated]
        }
        _ => Vec::new(),
    };
    CanonicalRoleSet::from_roles(roles)
}

fn rust_query_pack(adapter_schema: &str) -> LanguageQueryPack {
    let capture = |name, roles: &[CanonicalRole]| {
        QueryCaptureDeclaration::new(name, CanonicalRoleSet::from_roles(roles.iter().copied()))
            .expect("the Rust query capture is valid")
    };
    LanguageQueryPack::new(
        adapter_schema,
        vec![
            QueryFamilyDeclaration::provided(
                QueryFamily::Declarations,
                CapabilityAuthority::Adapter,
                "[(function_item) (struct_item) (enum_item) (union_item) (trait_item) (impl_item) (type_item) (const_item) (static_item) (mod_item) (macro_definition) (use_declaration)] @declaration",
                vec![capture("declaration", &[CanonicalRole::Declaration])],
            ),
            QueryFamilyDeclaration::provided(
                QueryFamily::References,
                CapabilityAuthority::Adapter,
                "(call_expression function: (_) @reference)",
                vec![capture(
                    "reference",
                    &[CanonicalRole::Expression, CanonicalRole::Read],
                )],
            ),
            QueryFamilyDeclaration::provided(
                QueryFamily::Scopes,
                CapabilityAuthority::Adapter,
                "(source_file) @scope.module\n(block) @scope.block",
                vec![
                    capture("scope.module", &[CanonicalRole::Module]),
                    capture("scope.block", &[CanonicalRole::Block]),
                ],
            ),
            QueryFamilyDeclaration::provided(
                QueryFamily::Control,
                CapabilityAuthority::Adapter,
                "[(if_expression) (match_expression)] @control.branch\n[(loop_expression) (while_expression) (for_expression)] @control.loop",
                vec![
                    capture(
                        "control.branch",
                        &[CanonicalRole::Expression, CanonicalRole::Branch],
                    ),
                    capture(
                        "control.loop",
                        &[CanonicalRole::Expression, CanonicalRole::Loop],
                    ),
                ],
            ),
            QueryFamilyDeclaration::provided(
                QueryFamily::Comments,
                CapabilityAuthority::Adapter,
                "[(line_comment) (block_comment)] @comment",
                vec![capture("comment", &[CanonicalRole::Comment])],
            ),
            QueryFamilyDeclaration::provided(
                QueryFamily::OpaqueGenerated,
                CapabilityAuthority::Adapter,
                "[(macro_invocation) (macro_definition)] @opaque",
                vec![capture("opaque", &[CanonicalRole::OpaqueRegion])],
            ),
            // No Rust contract query yet: an honest per-language gap.
            QueryFamilyDeclaration::unknown(QueryFamily::Contract),
        ],
    )
    .expect("the Rust query pack is valid")
}

fn rust_lexical_policy(adapter_schema: &str) -> LanguageLexicalPolicy {
    let token =
        |raw_kind, class| LexicalRule::new(raw_kind, None, LexicalClassification::token(class));
    let operator = |raw_kind: &'static str, text: Option<&str>, class: LexicalOperatorClass| {
        LexicalRule::new(
            raw_kind,
            text.map(str::to_string),
            LexicalClassification::operator(class),
        )
    };
    let mut rules = vec![
        operator("+", None, LexicalOperatorClass::Arithmetic),
        operator("-", None, LexicalOperatorClass::Arithmetic),
        operator("*", Some("*"), LexicalOperatorClass::Arithmetic),
        operator("/", None, LexicalOperatorClass::Arithmetic),
        operator("%", None, LexicalOperatorClass::Arithmetic),
        operator("==", None, LexicalOperatorClass::Comparison),
        operator("!=", None, LexicalOperatorClass::Comparison),
        operator("<", None, LexicalOperatorClass::Comparison),
        operator(">", None, LexicalOperatorClass::Comparison),
        operator("<=", None, LexicalOperatorClass::Comparison),
        operator(">=", None, LexicalOperatorClass::Comparison),
        operator("&&", None, LexicalOperatorClass::Logical),
        operator("||", None, LexicalOperatorClass::Logical),
        operator("!", None, LexicalOperatorClass::Logical),
        operator("=", None, LexicalOperatorClass::Assignment),
        operator("+=", None, LexicalOperatorClass::Assignment),
        operator("-=", None, LexicalOperatorClass::Assignment),
        operator("*=", None, LexicalOperatorClass::Assignment),
        operator("/=", None, LexicalOperatorClass::Assignment),
        operator("%=", None, LexicalOperatorClass::Assignment),
        operator("&", None, LexicalOperatorClass::Bitwise),
        operator("|", None, LexicalOperatorClass::Bitwise),
        operator("^", None, LexicalOperatorClass::Bitwise),
        operator("<<", None, LexicalOperatorClass::Bitwise),
        operator(">>", None, LexicalOperatorClass::Bitwise),
        operator(".", None, LexicalOperatorClass::MemberAccess),
        operator("::", None, LexicalOperatorClass::MemberAccess),
        operator("..", None, LexicalOperatorClass::Range),
        operator("..=", None, LexicalOperatorClass::Range),
    ];
    rules.extend(
        [
            "identifier",
            "field_identifier",
            "type_identifier",
            "lifetime",
        ]
        .into_iter()
        .map(|kind| token(kind, LexicalTokenClass::Identifier)),
    );
    rules.extend(
        [
            "integer_literal",
            "float_literal",
            "char_literal",
            "string_literal",
            "raw_string_literal",
            "boolean_literal",
            "true",
            "false",
        ]
        .into_iter()
        .map(|kind| token(kind, LexicalTokenClass::Literal)),
    );
    rules.extend(
        [
            "fn", "let", "mut", "pub", "struct", "enum", "union", "trait", "impl", "type", "const",
            "static", "mod", "use", "as", "where", "if", "else", "match", "loop", "while", "for",
            "in", "return", "break", "continue", "move", "async", "await", "unsafe", "extern",
            "crate", "super", "self", "Self", "dyn", "ref",
        ]
        .into_iter()
        .map(|kind| token(kind, LexicalTokenClass::Keyword)),
    );
    rules.extend(
        ["(", ")", "[", "]", "{", "}"]
            .into_iter()
            .map(|kind| token(kind, LexicalTokenClass::Delimiter)),
    );
    rules.extend(
        [";", ",", ":", "->", "=>", "?", "@", "#"]
            .into_iter()
            .map(|kind| token(kind, LexicalTokenClass::Punctuation)),
    );
    rules.extend([
        token("line_comment", LexicalTokenClass::Comment),
        token("block_comment", LexicalTokenClass::Comment),
        token("ERROR", LexicalTokenClass::Error),
        token("*", LexicalTokenClass::Other),
    ]);
    LanguageLexicalPolicy::provided(
        adapter_schema,
        CapabilityAuthority::Adapter,
        IdentifierCasePolicy::Sensitive,
        true,
        vec!["//".to_string()],
        vec![BlockCommentDelimiter::new("/*", "*/", true)],
        rules,
    )
    .expect("the Rust lexical policy is valid")
}

fn rust_construct_policy(adapter_schema: &str) -> LanguageConstructPolicy {
    LanguageConstructPolicy::new(
        adapter_schema,
        ParseRecoveryPolicy::provided(
            CapabilityAuthority::Syntax,
            ParseRecoveryHandling::FileIncomplete,
        ),
        vec![
            ConstructPolicySection::provided(
                ConstructPolicyKind::UnsupportedConstruct,
                CapabilityAuthority::Adapter,
                vec![ConstructRule::new(
                    "unsafe_block",
                    None,
                    ConstructHandling::Opaque,
                )],
            )
            .expect("the Rust unsupported policy is valid"),
            ConstructPolicySection::provided(
                ConstructPolicyKind::Macro,
                CapabilityAuthority::Adapter,
                vec![
                    ConstructRule::new("macro_invocation", None, ConstructHandling::Opaque),
                    ConstructRule::new("macro_definition", None, ConstructHandling::Opaque),
                ],
            )
            .expect("the Rust macro policy is valid"),
            ConstructPolicySection::provided(
                ConstructPolicyKind::GeneratedCode,
                CapabilityAuthority::Adapter,
                vec![
                    ConstructRule::new(
                        "attribute_item",
                        Some("#[generated]".to_string()),
                        ConstructHandling::SurfaceSyntax,
                    ),
                    ConstructRule::new(
                        "attribute_item",
                        Some("#[automatically_derived]".to_string()),
                        ConstructHandling::SurfaceSyntax,
                    ),
                ],
            )
            .expect("the Rust generated policy is valid"),
        ],
        DialectPolicy::provided(
            CapabilityAuthority::Syntax,
            vec![DialectDeclaration::new(
                "rust",
                "tree-sitter-rust",
                "0.24.2",
            )],
        )
        .expect("the Rust dialect policy is valid"),
    )
    .expect("the Rust construct policy is valid")
}

fn rust_control_flow_rule_pack(adapter_schema: &str) -> LanguageControlFlowRulePack {
    let selector = |raw_kind| ControlFlowSyntaxSelector::new(raw_kind, None);
    LanguageControlFlowRulePack::provided(
        adapter_schema,
        CapabilityAuthority::Adapter,
        vec![DialectDeclaration::new(
            "rust",
            "tree-sitter-rust",
            "0.24.2",
        )],
        ControlEvaluationOrder::LeftToRight,
        vec![
            ControlFlowOwnerRule::new(
                selector("function_item"),
                ControlFlowOwnerRuleKind::Callable,
                "body",
            ),
            ControlFlowOwnerRule::new(
                selector("closure_expression"),
                ControlFlowOwnerRuleKind::Callable,
                "body",
            ),
            ControlFlowOwnerRule::new(
                selector("const_item"),
                ControlFlowOwnerRuleKind::Initializer,
                "value",
            ),
            ControlFlowOwnerRule::new(
                selector("static_item"),
                ControlFlowOwnerRuleKind::Initializer,
                "value",
            ),
        ],
        vec![
            ControlFlowRule::new(selector("block"), ControlFlowAction::Sequence),
            ControlFlowRule::new(
                selector("let_declaration"),
                ControlFlowAction::NestedValue {
                    value_field: "value".into(),
                    unsupported_field: Some("alternative".into()),
                },
            ),
            ControlFlowRule::new(
                selector("expression_statement"),
                ControlFlowAction::Sequence,
            ),
            ControlFlowRule::new(selector("else_clause"), ControlFlowAction::Sequence),
            ControlFlowRule::new(
                selector("if_expression"),
                ControlFlowAction::Branch {
                    condition_field: "condition".into(),
                    consequence_field: "consequence".into(),
                    alternative_field: Some("alternative".into()),
                },
            ),
            ControlFlowRule::new(
                selector("match_expression"),
                ControlFlowAction::Match {
                    subject_field: "value".into(),
                    arm_kind: "match_arm".into(),
                    arm_body_field: Some("value".into()),
                    guard_field: None,
                },
            ),
            ControlFlowRule::new(
                selector("loop_expression"),
                ControlFlowAction::Loop {
                    form: ControlLoopForm::Infinite,
                    condition_field: None,
                    body_field: "body".into(),
                    alternative_field: None,
                    label_kind: Some("label".into()),
                },
            ),
            ControlFlowRule::new(
                selector("while_expression"),
                ControlFlowAction::Loop {
                    form: ControlLoopForm::PreTest,
                    condition_field: Some("condition".into()),
                    body_field: "body".into(),
                    alternative_field: None,
                    label_kind: Some("label".into()),
                },
            ),
            ControlFlowRule::new(
                selector("for_expression"),
                ControlFlowAction::Loop {
                    form: ControlLoopForm::Iterator,
                    condition_field: Some("value".into()),
                    body_field: "body".into(),
                    alternative_field: None,
                    label_kind: Some("label".into()),
                },
            ),
            ControlFlowRule::new(
                selector("return_expression"),
                ControlFlowAction::Abrupt {
                    form: ControlAbruptForm::Return,
                    value_field: None,
                    label_kind: None,
                },
            ),
            ControlFlowRule::new(
                selector("break_expression"),
                ControlFlowAction::Abrupt {
                    form: ControlAbruptForm::Break,
                    value_field: None,
                    label_kind: Some("label".into()),
                },
            ),
            ControlFlowRule::new(
                selector("continue_expression"),
                ControlFlowAction::Abrupt {
                    form: ControlAbruptForm::Continue,
                    value_field: None,
                    label_kind: Some("label".into()),
                },
            ),
            ControlFlowRule::new(
                selector("macro_invocation"),
                ControlFlowAction::OpaqueBoundary {
                    reason: "Rust macro expansion is unavailable".into(),
                },
            ),
            ControlFlowRule::new(
                selector("unsafe_block"),
                ControlFlowAction::OpaqueBoundary {
                    reason: "Rust unsafe control effects are not classified".into(),
                },
            ),
            ControlFlowRule::new(
                selector("call_expression"),
                ControlFlowAction::OpaqueBoundary {
                    reason: "Rust call unwind behavior requires callee effects and panic strategy"
                        .into(),
                },
            ),
            ControlFlowRule::new(
                selector("try_expression"),
                ControlFlowAction::OpaqueBoundary {
                    reason: "Rust question-mark propagation is not lowered yet".into(),
                },
            ),
            ControlFlowRule::new(
                selector("await_expression"),
                ControlFlowAction::Suspension {
                    form: ControlSuspensionForm::Await,
                    operand_field: Some("value".into()),
                },
            ),
            ControlFlowRule::new(
                selector("yield_expression"),
                ControlFlowAction::Suspension {
                    form: ControlSuspensionForm::Yield,
                    operand_field: Some("value".into()),
                },
            ),
        ],
    )
    .expect("the Rust control-flow rule pack is valid")
}

impl LangPack for RustPack {
    fn name(&self) -> &'static str {
        "rust"
    }

    fn capability_manifest(&self) -> LanguageAdapterCapabilityManifest {
        LanguageAdapterCapabilityManifest::current_syntax(self.adapter_schema())
            .with_declaration(CapabilityDeclaration::provided(
                AdapterCapability::CanonicalRoles,
                CapabilityAuthority::Adapter,
            ))
            .expect("the Rust S0 capability declaration is valid")
            .with_declaration(CapabilityDeclaration::provided(
                AdapterCapability::ControlFlow,
                CapabilityAuthority::Adapter,
            ))
            .expect("the Rust ControlFlow capability declaration is valid")
    }

    fn canonical_roles(&self, node: Node<'_>, text: &str) -> CanonicalRoleSet {
        rust_canonical_roles(node, text)
    }

    fn query_pack(&self) -> LanguageQueryPack {
        rust_query_pack(self.adapter_schema())
    }

    fn lexical_policy(&self) -> LanguageLexicalPolicy {
        rust_lexical_policy(self.adapter_schema())
    }

    fn construct_policy(&self) -> LanguageConstructPolicy {
        rust_construct_policy(self.adapter_schema())
    }

    fn resolution_rule_pack(&self) -> LanguageResolutionRulePack {
        crate::resolution::builtin_resolution_rule_pack(
            self.adapter_schema(),
            crate::resolution::BuiltinResolutionFamily::Rust,
        )
    }

    fn control_flow_rule_pack(&self) -> LanguageControlFlowRulePack {
        rust_control_flow_rule_pack(self.adapter_schema())
    }

    fn lang(&self) -> Lang {
        Lang::Rust
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["rs"]
    }

    fn grammar(&self) -> Option<tree_sitter::Language> {
        Some(tree_sitter_rust::LANGUAGE.into())
    }

    fn grammar_descriptor_for_path(&self, _path: &Path) -> Option<GrammarDescriptor> {
        Some(GrammarDescriptor {
            lang: Lang::Rust,
            dialect: "rust",
            grammar_id: "tree-sitter-rust",
            grammar_version: "0.24.2",
        })
    }

    fn line_comments(&self) -> &'static [&'static str] {
        &["//"]
    }

    fn metrics_regions(&self) -> &'static [&'static str] {
        &["function_item", "impl_item"]
    }

    fn metrics_branches(&self) -> &'static [&'static str] {
        &[
            "if_expression",
            "match_arm",
            "while_expression",
            "for_expression",
            "loop_expression",
        ]
    }

    fn metrics_nesting(&self) -> &'static [&'static str] {
        &[
            "if_expression",
            "match_expression",
            "while_expression",
            "for_expression",
            "loop_expression",
        ]
    }

    fn metrics_flow_breaks(&self) -> &'static [&'static str] {
        &[
            "return_expression",
            "break_expression",
            "continue_expression",
        ]
    }

    fn halstead_operator_tokens(&self) -> &'static [&'static str] {
        &[
            "=", "+", "-", "*", "/", "%", "==", "!=", "<", ">", "<=", ">=", "&&", "||", "!", "&",
            "|", "^", "<<", ">>", "+=", "-=", "*=", "/=", "%=", "if", "else", "match", "for",
            "while", "loop", "return", "break", "continue", "let",
        ]
    }

    fn region_class(&self, node: Node<'_>, _text: &str) -> RegionClass {
        match node.kind() {
            "block" => RegionClass::Behavioral,
            "attribute_item"
            | "enum_item"
            | "field_declaration"
            | "field_declaration_list"
            | "struct_item"
            | "trait_item"
            | "use_declaration" => RegionClass::Declaration,
            _ => RegionClass::Other,
        }
    }

    fn is_long_method_region(&self, node: Node<'_>, _text: &str) -> bool {
        node.kind() == "function_item"
    }

    fn is_constant_definition_region(&self, node: Node<'_>, _text: &str) -> bool {
        matches!(node.kind(), "const_item" | "static_item")
    }

    fn is_duplication_data_region(&self, node: Node<'_>, _text: &str) -> bool {
        matches!(
            node.kind(),
            "array_expression" | "struct_expression" | "field_initializer_list"
        ) || is_rust_data_macro_token_tree(node, _text)
    }

    fn tail_position_class(&self, node: Node<'_>, _text: &str) -> TailPositionClass {
        match node.kind() {
            "return_expression" => TailPositionClass::Return,
            "block"
                if node
                    .parent()
                    .is_some_and(|parent| parent.kind() == "function_item") =>
            {
                TailPositionClass::FunctionBody
            }
            _ => TailPositionClass::Other,
        }
    }

    fn enclosing_region(&self, node: Node<'_>, text: &str) -> Option<RegionSpan> {
        enclosing_rust_item(node, text)
    }
}

fn top_level_clojure_list(mut node: Node<'_>, text: &str) -> Option<RegionSpan> {
    let mut best = None;
    loop {
        if node.kind() == "list_lit" {
            best = Some(node);
        }
        let Some(parent) = node.parent() else {
            break;
        };
        if parent.kind() == "source" {
            break;
        }
        node = parent;
    }
    best.map(|node| region_from_node(node, text))
}

fn enclosing_julia_block(mut node: Node<'_>, text: &str) -> Option<RegionSpan> {
    loop {
        if matches!(
            node.kind(),
            "function_definition" | "struct_definition" | "module_definition"
        ) {
            return Some(region_from_node(node, text));
        }
        let parent = node.parent()?;
        node = parent;
    }
}

fn enclosing_rust_item(mut node: Node<'_>, text: &str) -> Option<RegionSpan> {
    loop {
        if matches!(node.kind(), "function_item" | "impl_item" | "mod_item") {
            return Some(region_from_node(node, text));
        }
        let parent = node.parent()?;
        node = parent;
    }
}

fn is_rust_data_macro_token_tree(node: Node<'_>, text: &str) -> bool {
    if node.kind() != "token_tree" {
        return false;
    }
    let Some(parent) = node.parent() else {
        return false;
    };
    if parent.kind() != "macro_invocation" {
        return false;
    }
    let Some(invocation) = text.get(parent.start_byte()..parent.end_byte()) else {
        return false;
    };
    let invocation = invocation.trim_start();
    invocation.starts_with("json!") || invocation.starts_with("vec!")
}

fn node_head_token<'a>(node: Node<'_>, text: &'a str) -> Option<&'a str> {
    let slice = text.get(node.start_byte()..node.end_byte())?;
    let trimmed = slice.trim_start_matches(|ch: char| {
        ch.is_whitespace() || matches!(ch, '(' | '[' | '{' | '#' | '\'' | '`')
    });
    let end = trimmed
        .char_indices()
        .find_map(|(idx, ch)| (!is_head_continue(ch)).then_some(idx))
        .unwrap_or(trimmed.len());
    (!trimmed[..end].is_empty()).then_some(&trimmed[..end])
}

fn is_head_continue(ch: char) -> bool {
    ch.is_ascii_alphanumeric()
        || matches!(
            ch,
            '_' | '-' | '?' | '!' | '*' | '+' | '/' | '<' | '>' | '=' | '.'
        )
}

fn region_from_node(node: Node<'_>, text: &str) -> RegionSpan {
    let start_position = node.start_position();
    let end_position = node.end_position();
    let mut end_line = end_position.row + 1;
    if end_position.column == 0 && end_line > start_position.row + 1 {
        end_line -= 1;
    }
    RegionSpan {
        start_line: start_position.row + 1,
        end_line,
        start_byte: node.start_byte(),
        end_byte: node.end_byte().min(text.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_detects_pack_by_extension() {
        let registry = Registry::default();
        assert_eq!(
            registry.pack_for_path(Path::new("sample.rs")).lang(),
            Lang::Rust
        );
        assert_eq!(
            registry.pack_for_path(Path::new("sample.unknown")).lang(),
            Lang::Generic
        );
        for extension in ["js", "jsx"] {
            assert_eq!(
                registry
                    .pack_for_path(Path::new(&format!("sample.{extension}")))
                    .lang(),
                Lang::JavaScript
            );
        }
        for extension in ["ts", "mts", "cts"] {
            assert_eq!(
                registry
                    .pack_for_path(Path::new(&format!("sample.{extension}")))
                    .lang(),
                Lang::TypeScript
            );
        }
        assert_eq!(
            registry.pack_for_path(Path::new("sample.tsx")).lang(),
            Lang::TypeScript
        );
    }

    #[test]
    fn canonical_role_catalog_is_composable_ordered_and_wire_pinned() {
        assert_eq!(CanonicalRole::ALL.len(), 23);
        let composed = CanonicalRoleSet::from_roles([
            CanonicalRole::Call,
            CanonicalRole::Callable,
            CanonicalRole::Declaration,
            CanonicalRole::Expression,
            CanonicalRole::Call,
        ]);
        assert_eq!(composed.len(), 4);
        assert!(composed.contains(CanonicalRole::Declaration));
        assert!(composed.contains(CanonicalRole::Callable));
        assert!(composed.contains(CanonicalRole::Expression));
        assert!(composed.contains(CanonicalRole::Call));
        assert_eq!(
            composed.iter().collect::<Vec<_>>(),
            [
                CanonicalRole::Declaration,
                CanonicalRole::Callable,
                CanonicalRole::Expression,
                CanonicalRole::Call,
            ]
        );

        let all = CanonicalRoleSet::from_roles(CanonicalRole::ALL);
        let value = serde_json::to_value(all).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "schema": "deslop.canonical-roles/1",
                "roles": [
                    "project", "module", "declaration", "type", "callable", "parameter",
                    "block", "statement", "expression", "branch", "loop", "match", "case",
                    "call", "read", "write", "literal", "comment", "import", "export",
                    "error", "generated", "opaque-region"
                ]
            })
        );
        assert_eq!(
            serde_json::from_value::<CanonicalRoleSet>(value).unwrap(),
            all
        );

        for malformed in [
            serde_json::json!({
                "schema": "deslop.canonical-roles/999",
                "roles": ["call"]
            }),
            serde_json::json!({
                "schema": "deslop.canonical-roles/1",
                "roles": ["call", "expression"]
            }),
            serde_json::json!({
                "schema": "deslop.canonical-roles/1",
                "roles": ["call", "call"]
            }),
        ] {
            assert!(serde_json::from_value::<CanonicalRoleSet>(malformed).is_err());
        }
    }

    #[test]
    fn language_query_pack_is_total_strict_and_wire_pinned() {
        let capture = QueryCaptureDeclaration::new(
            "declaration.callable",
            CanonicalRoleSet::from_roles([CanonicalRole::Declaration, CanonicalRole::Callable]),
        )
        .unwrap();
        let pack = LanguageQueryPack::new(
            "deslop-lang-adapter/query-test-1",
            QueryFamily::ALL
                .into_iter()
                .map(|family| {
                    if family == QueryFamily::Declarations {
                        QueryFamilyDeclaration::provided(
                            family,
                            CapabilityAuthority::Adapter,
                            "(function_item) @declaration.callable",
                            vec![capture.clone()],
                        )
                    } else {
                        QueryFamilyDeclaration::unknown(family)
                    }
                })
                .collect(),
        )
        .unwrap();
        assert_eq!(pack.schema(), LANGUAGE_QUERY_PACK_SCHEMA);
        assert_eq!(pack.queries().len(), 7);
        assert_eq!(
            serde_json::to_value(&pack).unwrap(),
            serde_json::json!({
                "schema": "deslop.language-query-pack/1",
                "adapter_schema": "deslop-lang-adapter/query-test-1",
                "queries": [
                    {
                        "family": "declarations",
                        "support": "provided",
                        "authority": "adapter",
                        "source": "(function_item) @declaration.callable",
                        "captures": [{
                            "name": "declaration.callable",
                            "roles": {
                                "schema": "deslop.canonical-roles/1",
                                "roles": ["declaration", "callable"]
                            }
                        }]
                    },
                    {"family":"references","support":"unknown","authority":null,"source":null,"captures":[]},
                    {"family":"scopes","support":"unknown","authority":null,"source":null,"captures":[]},
                    {"family":"control","support":"unknown","authority":null,"source":null,"captures":[]},
                    {"family":"comments","support":"unknown","authority":null,"source":null,"captures":[]},
                    {"family":"opaque-generated","support":"unknown","authority":null,"source":null,"captures":[]},
                    {"family":"contract","support":"unknown","authority":null,"source":null,"captures":[]}
                ]
            })
        );

        let value = serde_json::to_value(pack).unwrap();
        assert!(serde_json::from_value::<LanguageQueryPack>(value.clone()).is_ok());
        let mut missing = value.clone();
        missing["queries"].as_array_mut().unwrap().pop();
        assert!(serde_json::from_value::<LanguageQueryPack>(missing).is_err());
        let mut reordered = value.clone();
        reordered["queries"].as_array_mut().unwrap().swap(0, 1);
        assert!(serde_json::from_value::<LanguageQueryPack>(reordered).is_err());
        let mut missing_source = value.clone();
        missing_source["queries"][0]["source"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<LanguageQueryPack>(missing_source).is_err());
        let mut duplicate_capture = value;
        let duplicate = duplicate_capture["queries"][0]["captures"][0].clone();
        duplicate_capture["queries"][0]["captures"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
        assert!(serde_json::from_value::<LanguageQueryPack>(duplicate_capture).is_err());
    }

    #[test]
    fn lexical_policy_is_total_ordered_and_wire_pinned() {
        let policy = LanguageLexicalPolicy::provided(
            "deslop-lang-adapter/lexical-test-1",
            CapabilityAuthority::Adapter,
            IdentifierCasePolicy::Sensitive,
            true,
            vec!["//".to_string()],
            vec![BlockCommentDelimiter::new("/*", "*/", true)],
            vec![
                LexicalRule::new(
                    "identifier",
                    None,
                    LexicalClassification::token(LexicalTokenClass::Identifier),
                ),
                LexicalRule::new(
                    "==",
                    Some("==".to_string()),
                    LexicalClassification::operator(LexicalOperatorClass::Comparison),
                ),
                LexicalRule::new(
                    "*",
                    Some("*".to_string()),
                    LexicalClassification::operator(LexicalOperatorClass::Arithmetic),
                ),
                LexicalRule::new(
                    "line_comment",
                    None,
                    LexicalClassification::token(LexicalTokenClass::Comment),
                ),
                LexicalRule::new(
                    "*",
                    None,
                    LexicalClassification::token(LexicalTokenClass::Other),
                ),
            ],
        )
        .unwrap();
        assert_eq!(
            policy.classify("==", "==").unwrap().operator_class(),
            Some(LexicalOperatorClass::Comparison)
        );
        assert_eq!(
            policy.classify("*", "*").unwrap().operator_class(),
            Some(LexicalOperatorClass::Arithmetic)
        );
        assert_eq!(
            policy
                .classify("identifier", "value")
                .unwrap()
                .token_class(),
            LexicalTokenClass::Identifier
        );
        assert_eq!(
            policy.classify("unknown", "?").unwrap().token_class(),
            LexicalTokenClass::Other
        );
        assert!(policy.classify_explicit("unknown", "?").is_none());
        let value = serde_json::to_value(&policy).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "schema": "deslop.language-lexical-policy/1",
                "adapter_schema": "deslop-lang-adapter/lexical-test-1",
                "support": "provided",
                "authority": "adapter",
                "identifier_case": "sensitive",
                "unicode_identifiers": true,
                "line_comments": ["//"],
                "block_comments": [{"open": "/*", "close": "*/", "nested": true}],
                "rules": [
                    {
                        "raw_kind": "identifier",
                        "text": null,
                        "classification": {"token": "identifier", "operator": null}
                    },
                    {
                        "raw_kind": "==",
                        "text": "==",
                        "classification": {"token": "operator", "operator": "comparison"}
                    },
                    {
                        "raw_kind": "*",
                        "text": "*",
                        "classification": {"token": "operator", "operator": "arithmetic"}
                    },
                    {
                        "raw_kind": "line_comment",
                        "text": null,
                        "classification": {"token": "comment", "operator": null}
                    },
                    {
                        "raw_kind": "*",
                        "text": null,
                        "classification": {"token": "other", "operator": null}
                    }
                ]
            })
        );
        assert_eq!(
            serde_json::from_value::<LanguageLexicalPolicy>(value.clone()).unwrap(),
            policy
        );

        let mut no_fallback = value.clone();
        no_fallback["rules"].as_array_mut().unwrap().pop();
        assert!(serde_json::from_value::<LanguageLexicalPolicy>(no_fallback).is_err());
        let mut bad_operator = value;
        bad_operator["rules"][1]["classification"]["operator"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<LanguageLexicalPolicy>(bad_operator).is_err());

        let nonterminal_wildcard = LanguageLexicalPolicy::provided(
            "deslop-lang-adapter/lexical-test-1",
            CapabilityAuthority::Adapter,
            IdentifierCasePolicy::Sensitive,
            true,
            vec![],
            vec![],
            vec![
                LexicalRule::new(
                    "*",
                    None,
                    LexicalClassification::token(LexicalTokenClass::Other),
                ),
                LexicalRule::new(
                    "*",
                    None,
                    LexicalClassification::token(LexicalTokenClass::Error),
                ),
            ],
        )
        .unwrap_err();
        assert!(nonterminal_wildcard.contains("wildcard rule must be terminal"));

        let shadowed_exact_text = LanguageLexicalPolicy::provided(
            "deslop-lang-adapter/lexical-test-1",
            CapabilityAuthority::Adapter,
            IdentifierCasePolicy::Sensitive,
            true,
            vec![],
            vec![],
            vec![
                LexicalRule::new(
                    "identifier",
                    None,
                    LexicalClassification::token(LexicalTokenClass::Identifier),
                ),
                LexicalRule::new(
                    "identifier",
                    Some("contextual".to_string()),
                    LexicalClassification::token(LexicalTokenClass::Keyword),
                ),
                LexicalRule::new(
                    "*",
                    None,
                    LexicalClassification::token(LexicalTokenClass::Other),
                ),
            ],
        )
        .unwrap_err();
        assert!(shadowed_exact_text.contains("shadows a later exact-text rule"));

        for unavailable in [
            LanguageLexicalPolicy::unknown("deslop-lang-adapter/lexical-test-1"),
            LanguageLexicalPolicy::unsupported("deslop-lang-adapter/lexical-test-1"),
        ] {
            unavailable.validate().unwrap();
            assert!(unavailable.classify("identifier", "value").is_none());
            assert!(unavailable.rules().is_empty());
            assert_eq!(
                serde_json::from_value::<LanguageLexicalPolicy>(
                    serde_json::to_value(&unavailable).unwrap()
                )
                .unwrap(),
                unavailable
            );
        }
    }

    #[test]
    fn construct_policy_is_total_strict_and_wire_pinned() {
        let policy = LanguageConstructPolicy::new(
            "deslop-lang-adapter/construct-test-1",
            ParseRecoveryPolicy::provided(
                CapabilityAuthority::Syntax,
                ParseRecoveryHandling::FileIncomplete,
            ),
            vec![
                ConstructPolicySection::provided(
                    ConstructPolicyKind::UnsupportedConstruct,
                    CapabilityAuthority::Adapter,
                    vec![ConstructRule::new(
                        "unsafe_block",
                        None,
                        ConstructHandling::Opaque,
                    )],
                )
                .unwrap(),
                ConstructPolicySection::provided(
                    ConstructPolicyKind::Macro,
                    CapabilityAuthority::Adapter,
                    vec![ConstructRule::new(
                        "macro_invocation",
                        None,
                        ConstructHandling::Opaque,
                    )],
                )
                .unwrap(),
                ConstructPolicySection::provided(
                    ConstructPolicyKind::GeneratedCode,
                    CapabilityAuthority::Adapter,
                    vec![ConstructRule::new(
                        "attribute_item",
                        Some("#[generated]".to_string()),
                        ConstructHandling::SurfaceSyntax,
                    )],
                )
                .unwrap(),
            ],
            DialectPolicy::provided(
                CapabilityAuthority::Syntax,
                vec![DialectDeclaration::new(
                    "same-lang",
                    "tree-sitter-rust",
                    "test",
                )],
            )
            .unwrap(),
        )
        .unwrap();
        let value = serde_json::to_value(&policy).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "schema": "deslop.language-construct-policy/1",
                "adapter_schema": "deslop-lang-adapter/construct-test-1",
                "parse_recovery": {
                    "support": "provided",
                    "authority": "syntax",
                    "handling": "file-incomplete"
                },
                "constructs": [
                    {
                        "kind": "unsupported-construct",
                        "support": "provided",
                        "authority": "adapter",
                        "rules": [{
                            "raw_kind": "unsafe_block",
                            "text": null,
                            "handling": "opaque"
                        }]
                    },
                    {
                        "kind": "macro",
                        "support": "provided",
                        "authority": "adapter",
                        "rules": [{
                            "raw_kind": "macro_invocation",
                            "text": null,
                            "handling": "opaque"
                        }]
                    },
                    {
                        "kind": "generated-code",
                        "support": "provided",
                        "authority": "adapter",
                        "rules": [{
                            "raw_kind": "attribute_item",
                            "text": "#[generated]",
                            "handling": "surface-syntax"
                        }]
                    }
                ],
                "dialects": {
                    "support": "provided",
                    "authority": "syntax",
                    "variants": [{
                        "dialect": "same-lang",
                        "grammar_id": "tree-sitter-rust",
                        "grammar_version": "test"
                    }]
                }
            })
        );
        assert_eq!(
            serde_json::from_value::<LanguageConstructPolicy>(value.clone()).unwrap(),
            policy
        );
        assert!(
            policy
                .construct(ConstructPolicyKind::Macro)
                .matching_rule("macro_invocation", "vec![1]")
                .is_some()
        );
        assert!(
            policy
                .construct(ConstructPolicyKind::GeneratedCode)
                .matching_rule("attribute_item", "#[other]")
                .is_none()
        );
        assert!(
            policy
                .dialects()
                .declaration("same-lang", "tree-sitter-rust", "test")
                .is_some()
        );

        let mut incomplete = value.clone();
        incomplete["constructs"].as_array_mut().unwrap().pop();
        assert!(serde_json::from_value::<LanguageConstructPolicy>(incomplete).is_err());
        let mut reordered = value.clone();
        reordered["constructs"].as_array_mut().unwrap().swap(0, 1);
        assert!(serde_json::from_value::<LanguageConstructPolicy>(reordered).is_err());
        let mut unavailable_payload = value;
        unavailable_payload["constructs"][1]["support"] = serde_json::json!("unknown");
        assert!(serde_json::from_value::<LanguageConstructPolicy>(unavailable_payload).is_err());

        let shadowed = ConstructPolicySection::provided(
            ConstructPolicyKind::GeneratedCode,
            CapabilityAuthority::Adapter,
            vec![
                ConstructRule::new("attribute_item", None, ConstructHandling::SurfaceSyntax),
                ConstructRule::new(
                    "attribute_item",
                    Some("#[generated]".to_string()),
                    ConstructHandling::Opaque,
                ),
            ],
        )
        .unwrap_err();
        assert!(shadowed.contains("shadows a later exact-text rule"));

        let unknown = LanguageConstructPolicy::unknown("deslop-lang-adapter/construct-test-1");
        unknown.validate().unwrap();
        assert_eq!(
            unknown.parse_recovery().support(),
            CapabilitySupport::Unknown
        );
        assert_eq!(unknown.dialects().support(), CapabilitySupport::Unknown);
        assert!(
            unknown
                .constructs()
                .iter()
                .all(|section| section.support() == CapabilitySupport::Unknown
                    && section.rules().is_empty())
        );
    }

    #[test]
    fn capability_catalog_is_total_tiered_and_wire_pinned() {
        assert_eq!(
            SemanticTier::ALL.map(|tier| {
                AdapterCapability::ALL
                    .iter()
                    .filter(|capability| capability.tier() == tier)
                    .count()
            }),
            [6, 4, 6, 5, 2]
        );
        let manifest =
            LanguageAdapterCapabilityManifest::current_syntax("deslop-lang-adapter/test-1");
        assert_eq!(manifest.schema(), LANGUAGE_ADAPTER_CAPABILITY_SCHEMA);
        assert_eq!(manifest.highest_complete_tier(), None);
        assert_eq!(
            manifest.declaration(AdapterCapability::GrammarSelection),
            &CapabilityDeclaration::provided(
                AdapterCapability::GrammarSelection,
                CapabilityAuthority::Syntax
            )
        );
        assert_eq!(
            manifest.declaration(AdapterCapability::CanonicalRoles),
            &CapabilityDeclaration::unknown(AdapterCapability::CanonicalRoles)
        );
        assert_eq!(
            serde_json::to_value(&manifest).unwrap(),
            serde_json::json!({
                "schema": "deslop.language-adapter-capabilities/2",
                "adapter_schema": "deslop-lang-adapter/test-1",
                "capabilities": [
                    {"capability":"grammar-selection","support":"provided","authority":"syntax"},
                    {"capability":"lossless-syntax","support":"provided","authority":"syntax"},
                    {"capability":"canonical-roles","support":"unknown","authority":null},
                    {"capability":"source-spans","support":"provided","authority":"syntax"},
                    {"capability":"tokens","support":"provided","authority":"syntax"},
                    {"capability":"comments","support":"provided","authority":"adapter"},
                    {"capability":"regions","support":"provided","authority":"adapter"},
                    {"capability":"local-metrics","support":"provided","authority":"adapter"},
                    {"capability":"clone-normalization","support":"provided","authority":"adapter"},
                    {"capability":"syntactic-recipes","support":"provided","authority":"adapter"},
                    {"capability":"lexical-scopes","support":"unknown","authority":null},
                    {"capability":"name-resolution","support":"unknown","authority":null},
                    {"capability":"control-flow","support":"unknown","authority":null},
                    {"capability":"def-use","support":"unknown","authority":null},
                    {"capability":"effects","support":"unknown","authority":null},
                    {"capability":"local-pdg","support":"unknown","authority":null},
                    {"capability":"imports-exports","support":"unknown","authority":null},
                    {"capability":"call-graph","support":"unknown","authority":null},
                    {"capability":"dependency-graph","support":"unknown","authority":null},
                    {"capability":"sdg","support":"unknown","authority":null},
                    {"capability":"api-impact","support":"unknown","authority":null},
                    {"capability":"compiler-type-evidence","support":"unknown","authority":null},
                    {"capability":"targeted-dynamic-verification","support":"unknown","authority":null}
                ]
            })
        );
    }

    #[test]
    fn complete_tier_is_derived_and_manifest_validation_rejects_gaps() {
        let complete_through = |tier| {
            LanguageAdapterCapabilityManifest::new(
                "deslop-lang-adapter/tier-test",
                AdapterCapability::ALL
                    .into_iter()
                    .map(|capability| {
                        if capability.tier() <= tier {
                            CapabilityDeclaration::provided(
                                capability,
                                CapabilityAuthority::Adapter,
                            )
                        } else {
                            CapabilityDeclaration::unknown(capability)
                        }
                    })
                    .collect(),
            )
            .unwrap()
        };
        for tier in SemanticTier::ALL {
            assert_eq!(complete_through(tier).highest_complete_tier(), Some(tier));
        }

        let manifest = complete_through(SemanticTier::S0);
        let mut missing = serde_json::to_value(&manifest).unwrap();
        missing["capabilities"].as_array_mut().unwrap().pop();
        assert!(
            serde_json::from_value::<LanguageAdapterCapabilityManifest>(missing)
                .unwrap_err()
                .to_string()
                .contains("expected 23")
        );

        let mut authority = serde_json::to_value(&manifest).unwrap();
        authority["capabilities"][0]["authority"] = serde_json::Value::Null;
        assert!(
            serde_json::from_value::<LanguageAdapterCapabilityManifest>(authority)
                .unwrap_err()
                .to_string()
                .contains("no authority")
        );

        let mut reordered = serde_json::to_value(&manifest).unwrap();
        reordered["capabilities"].as_array_mut().unwrap().swap(0, 1);
        assert!(
            serde_json::from_value::<LanguageAdapterCapabilityManifest>(reordered)
                .unwrap_err()
                .to_string()
                .contains("not total")
        );
    }

    #[test]
    fn every_registered_pack_has_a_valid_honest_manifest() {
        let registry = Registry::default();
        for pack in registry
            .packs
            .iter()
            .copied()
            .chain(std::iter::once(registry.generic))
        {
            let manifest = pack.capability_manifest();
            manifest.validate().unwrap();
            assert_eq!(manifest.adapter_schema(), pack.adapter_schema());
            assert_eq!(manifest.capabilities().len(), 23);
            let production_policy = matches!(
                pack.lang(),
                Lang::JavaScript
                    | Lang::TypeScript
                    | Lang::Python
                    | Lang::Clojure
                    | Lang::Julia
                    | Lang::Rust
            );
            assert_eq!(
                manifest.highest_complete_tier(),
                production_policy.then_some(SemanticTier::S1)
            );
            let queries = pack.query_pack();
            queries.validate().unwrap();
            assert_eq!(queries.adapter_schema(), pack.adapter_schema());
            assert_eq!(queries.queries().len(), 7);
            let expected = if production_policy {
                CapabilitySupport::Provided
            } else {
                CapabilitySupport::Unknown
            };
            assert!(queries.queries().iter().all(|query| {
                let expected_query = if pack.lang() == Lang::Clojure
                    && matches!(
                        query.family(),
                        QueryFamily::Declarations | QueryFamily::References | QueryFamily::Control
                    ) {
                    CapabilitySupport::Unknown
                } else if query.family() == QueryFamily::Contract {
                    // Contract facts are provided by the Python, Julia, and
                    // plain-JavaScript adapters; everywhere else the family
                    // is an honest per-language capability gap.
                    if matches!(pack.lang(), Lang::Python | Lang::Julia | Lang::JavaScript)
                        && production_policy
                    {
                        CapabilitySupport::Provided
                    } else {
                        CapabilitySupport::Unknown
                    }
                } else {
                    expected
                };
                query.support() == expected_query
            }));
            let lexical = pack.lexical_policy();
            lexical.validate().unwrap();
            assert_eq!(lexical.adapter_schema(), pack.adapter_schema());
            assert_eq!(lexical.support(), expected);
            assert_eq!(lexical.rules().is_empty(), !production_policy);
            let constructs = pack.construct_policy();
            constructs.validate().unwrap();
            assert_eq!(constructs.adapter_schema(), pack.adapter_schema());
            assert_eq!(constructs.parse_recovery().support(), expected);
            assert_eq!(constructs.dialects().support(), expected);
            assert!(constructs.constructs().iter().all(|section| {
                let expected_section = if matches!(
                    pack.lang(),
                    Lang::JavaScript | Lang::TypeScript | Lang::Python
                ) && section.kind() == ConstructPolicyKind::Macro
                {
                    CapabilitySupport::Unsupported
                } else {
                    expected
                };
                section.support() == expected_section
            }));
            let control_flow = pack.control_flow_rule_pack();
            control_flow.validate().unwrap();
            assert_eq!(control_flow.adapter_schema(), pack.adapter_schema());
            let declaration = manifest.declaration(AdapterCapability::ControlFlow);
            assert_eq!(control_flow.support(), declaration.support());
            assert_eq!(control_flow.authority(), declaration.authority());
            if pack.lang() == Lang::Rust {
                assert_eq!(control_flow.support(), CapabilitySupport::Provided);
                assert_eq!(control_flow.authority(), Some(CapabilityAuthority::Adapter));
                assert_eq!(control_flow.dialects().len(), 1);
                assert_eq!(control_flow.owners().len(), 4);
                assert_eq!(control_flow.rules().len(), 18);
            } else {
                assert!(control_flow.dialects().is_empty());
                assert!(control_flow.owners().is_empty());
                assert!(control_flow.rules().is_empty());
            }
        }
    }
}
