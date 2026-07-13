use std::path::Path;

use anyhow::Result;
use deslop_core::Lang;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use tree_sitter::Node;

pub const LANGUAGE_ADAPTER_CAPABILITY_SCHEMA: &str = "deslop.language-adapter-capabilities/1";
pub const CANONICAL_ROLE_SCHEMA: &str = "deslop.canonical-roles/1";

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
    Compiler,
    RuntimeVerification,
}

impl CapabilityAuthority {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::Adapter => "adapter",
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
        "deslop-lang-adapter/1"
    }
    fn capability_manifest(&self) -> LanguageAdapterCapabilityManifest;
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

impl LangPack for ClojurePack {
    fn name(&self) -> &'static str {
        "clojure"
    }

    fn capability_manifest(&self) -> LanguageAdapterCapabilityManifest {
        LanguageAdapterCapabilityManifest::current_syntax(self.adapter_schema())
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
                    node_head_token(node, text),
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
            && matches!(node_head_token(node, text), Some("throw" | "recur"))
    }

    fn region_class(&self, node: Node<'_>, text: &str) -> RegionClass {
        if node.kind() != "list_lit" {
            return RegionClass::Other;
        }
        match node_head_token(node, text) {
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
        node.kind() == "list_lit" && matches!(node_head_token(node, text), Some("def" | "defonce"))
    }

    fn is_duplication_data_region(&self, node: Node<'_>, _text: &str) -> bool {
        matches!(node.kind(), "map_lit" | "set_lit")
    }

    fn enclosing_region(&self, node: Node<'_>, text: &str) -> Option<RegionSpan> {
        top_level_clojure_list(node, text)
    }
}

fn clojure_form_is_evaluated(node: Node<'_>) -> bool {
    if node.kind() != "list_lit" {
        return false;
    }
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

impl LangPack for JuliaPack {
    fn name(&self) -> &'static str {
        "julia"
    }

    fn capability_manifest(&self) -> LanguageAdapterCapabilityManifest {
        LanguageAdapterCapabilityManifest::current_syntax(self.adapter_schema())
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

impl LangPack for PythonPack {
    fn name(&self) -> &'static str {
        "python"
    }

    fn capability_manifest(&self) -> LanguageAdapterCapabilityManifest {
        LanguageAdapterCapabilityManifest::current_syntax(self.adapter_schema())
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

impl LangPack for JavaScriptPack {
    fn name(&self) -> &'static str {
        "javascript"
    }

    fn capability_manifest(&self) -> LanguageAdapterCapabilityManifest {
        LanguageAdapterCapabilityManifest::current_syntax(self.adapter_schema())
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

impl LangPack for RustPack {
    fn name(&self) -> &'static str {
        "rust"
    }

    fn capability_manifest(&self) -> LanguageAdapterCapabilityManifest {
        LanguageAdapterCapabilityManifest::current_syntax(self.adapter_schema())
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
                "schema": "deslop.language-adapter-capabilities/1",
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
            assert_eq!(manifest.highest_complete_tier(), None);
        }
    }
}
