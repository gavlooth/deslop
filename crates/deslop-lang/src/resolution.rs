use std::collections::BTreeSet;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{CapabilityAuthority, CapabilitySupport, DialectDeclaration};

pub const LANGUAGE_RESOLUTION_RULE_SCHEMA: &str = "deslop.resolution-rules/1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResolutionRuleSectionKind {
    ScopeModel,
    Extraction,
    Namespaces,
    VisibilityTiming,
    ShadowingDuplicates,
    Qualification,
    ImportsExports,
    ModuleMapping,
    DynamicBoundaries,
    Precedence,
}

impl ResolutionRuleSectionKind {
    pub const ALL: [Self; 10] = [
        Self::ScopeModel,
        Self::Extraction,
        Self::Namespaces,
        Self::VisibilityTiming,
        Self::ShadowingDuplicates,
        Self::Qualification,
        Self::ImportsExports,
        Self::ModuleMapping,
        Self::DynamicBoundaries,
        Self::Precedence,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ScopeModel => "scope-model",
            Self::Extraction => "extraction",
            Self::Namespaces => "namespaces",
            Self::VisibilityTiming => "visibility-timing",
            Self::ShadowingDuplicates => "shadowing-duplicates",
            Self::Qualification => "qualification",
            Self::ImportsExports => "imports-exports",
            Self::ModuleMapping => "module-mapping",
            Self::DynamicBoundaries => "dynamic-boundaries",
            Self::Precedence => "precedence",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionSyntaxSelector {
    raw_kind: String,
    field: Option<String>,
    exact_text: Option<String>,
}

impl ResolutionSyntaxSelector {
    pub fn new(
        raw_kind: impl Into<String>,
        field: Option<String>,
        exact_text: Option<String>,
    ) -> Result<Self, String> {
        let value = Self {
            raw_kind: raw_kind.into(),
            field,
            exact_text,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn raw_kind(&self) -> &str {
        &self.raw_kind
    }

    pub fn field(&self) -> Option<&str> {
        self.field.as_deref()
    }

    pub fn exact_text(&self) -> Option<&str> {
        self.exact_text.as_deref()
    }

    fn validate(&self) -> Result<(), String> {
        validate_text("resolution selector raw kind", &self.raw_kind)?;
        validate_optional_text("resolution selector field", self.field.as_deref())?;
        validate_optional_text("resolution selector text", self.exact_text.as_deref())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuleNamespace {
    Value,
    Type,
    Module,
    Macro,
    Label,
    Member,
    AdapterDefined { schema: String, name: String },
}

impl RuleNamespace {
    fn validate(&self) -> Result<(), String> {
        if let Self::AdapterDefined { schema, name } = self {
            validate_text("adapter namespace schema", schema)?;
            validate_text("adapter namespace name", name)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuleScopeKind {
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScopeParentRule {
    NearestDeclaredScope,
    NearestModule,
    NearestCallable,
    BuildContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExtractionFactKind {
    Declaration,
    Definition,
    Binding,
    Reference,
    Import,
    Export,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeclarationTimingRule {
    ScopeEntry,
    SourceOrder,
    AfterInitializer,
    Hoisted,
    Recursive,
    TemporalDeadZone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DuplicateDefinitionRule {
    Ambiguous,
    MergeDeclarations,
    LatestVisible,
    AdapterRejects,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QualificationRootRule {
    Lexical,
    CurrentModule,
    ParentModule,
    PackageRoot,
    Receiver,
    Type,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImportTraversalRule {
    Explicit,
    Selective,
    Alias,
    Glob,
    Prelude,
    Export,
    ReExport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModulePrerequisite {
    PackageManifest,
    Lockfile,
    BuildTarget,
    SourceRoots,
    GeneratedRoots,
    ModuleMap,
    Features,
    Platform,
    LanguageMode,
    Dependencies,
    Prelude,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrecedenceDimension {
    RuleStep,
    LexicalDistance,
    Namespace,
    ImportSpecificity,
    SourceOrder,
    AdapterOrder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrecedenceDirection {
    LowerFirst,
    HigherFirst,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrecedenceTerm {
    dimension: PrecedenceDimension,
    direction: PrecedenceDirection,
}

impl PrecedenceTerm {
    pub const fn new(dimension: PrecedenceDimension, direction: PrecedenceDirection) -> Self {
        Self {
            dimension,
            direction,
        }
    }

    pub fn dimension(self) -> PrecedenceDimension {
        self.dimension
    }

    pub fn direction(self) -> PrecedenceDirection {
        self.direction
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ResolutionInstruction {
    CreateScope {
        selector: ResolutionSyntaxSelector,
        scope_kind: RuleScopeKind,
        parent: ScopeParentRule,
    },
    ExtractFact {
        selector: ResolutionSyntaxSelector,
        fact_kind: ExtractionFactKind,
        name_field: Option<String>,
        namespace: Option<RuleNamespace>,
    },
    DeclareNamespace {
        namespace: RuleNamespace,
    },
    UnifyNamespaces {
        namespaces: Vec<RuleNamespace>,
    },
    AllowNamespaceTransition {
        from: RuleNamespace,
        to: RuleNamespace,
        rule: String,
    },
    Visibility {
        selector: ResolutionSyntaxSelector,
        rule: String,
    },
    DeclarationTiming {
        selector: ResolutionSyntaxSelector,
        timing: DeclarationTimingRule,
    },
    Shadowing {
        inner: RuleNamespace,
        outer: RuleNamespace,
        rule: String,
    },
    DuplicateDefinitions {
        namespace: RuleNamespace,
        rule: DuplicateDefinitionRule,
    },
    QualificationRoot {
        token: String,
        root: QualificationRootRule,
    },
    MemberTraversal {
        separator: String,
        namespace: RuleNamespace,
    },
    ImportTraversal {
        rule: ImportTraversalRule,
    },
    RequireModuleInput {
        prerequisite: ModulePrerequisite,
    },
    DynamicBoundary {
        selector: ResolutionSyntaxSelector,
        namespaces: Vec<RuleNamespace>,
        reason: String,
    },
    Precedence {
        terms: Vec<PrecedenceTerm>,
    },
}

impl ResolutionInstruction {
    pub fn section(&self) -> ResolutionRuleSectionKind {
        match self {
            Self::CreateScope { .. } => ResolutionRuleSectionKind::ScopeModel,
            Self::ExtractFact { .. } => ResolutionRuleSectionKind::Extraction,
            Self::DeclareNamespace { .. }
            | Self::UnifyNamespaces { .. }
            | Self::AllowNamespaceTransition { .. } => ResolutionRuleSectionKind::Namespaces,
            Self::Visibility { .. } | Self::DeclarationTiming { .. } => {
                ResolutionRuleSectionKind::VisibilityTiming
            }
            Self::Shadowing { .. } | Self::DuplicateDefinitions { .. } => {
                ResolutionRuleSectionKind::ShadowingDuplicates
            }
            Self::QualificationRoot { .. } | Self::MemberTraversal { .. } => {
                ResolutionRuleSectionKind::Qualification
            }
            Self::ImportTraversal { .. } => ResolutionRuleSectionKind::ImportsExports,
            Self::RequireModuleInput { .. } => ResolutionRuleSectionKind::ModuleMapping,
            Self::DynamicBoundary { .. } => ResolutionRuleSectionKind::DynamicBoundaries,
            Self::Precedence { .. } => ResolutionRuleSectionKind::Precedence,
        }
    }

    fn validate(&self) -> Result<(), String> {
        match self {
            Self::CreateScope { selector, .. } | Self::DeclarationTiming { selector, .. } => {
                selector.validate()
            }
            Self::ExtractFact {
                selector,
                name_field,
                namespace,
                ..
            } => {
                selector.validate()?;
                validate_optional_text("extraction name field", name_field.as_deref())?;
                if let Some(namespace) = namespace {
                    namespace.validate()?;
                }
                Ok(())
            }
            Self::DeclareNamespace { namespace } | Self::DuplicateDefinitions { namespace, .. } => {
                namespace.validate()
            }
            Self::UnifyNamespaces { namespaces } => {
                if namespaces.len() < 2 {
                    return Err("namespace unification requires at least two namespaces".into());
                }
                validate_namespaces(namespaces)
            }
            Self::AllowNamespaceTransition { from, to, rule }
            | Self::Shadowing {
                inner: from,
                outer: to,
                rule,
            } => {
                from.validate()?;
                to.validate()?;
                validate_text("resolution relation rule", rule)
            }
            Self::Visibility { selector, rule } => {
                selector.validate()?;
                validate_text("visibility rule", rule)
            }
            Self::QualificationRoot { token, .. } => {
                validate_text("qualification root token", token)
            }
            Self::MemberTraversal {
                separator,
                namespace,
            } => {
                validate_text("member separator", separator)?;
                namespace.validate()
            }
            Self::ImportTraversal { .. } | Self::RequireModuleInput { .. } => Ok(()),
            Self::DynamicBoundary {
                selector,
                namespaces,
                reason,
            } => {
                selector.validate()?;
                if namespaces.is_empty() {
                    return Err("dynamic boundary must name affected namespaces".into());
                }
                validate_namespaces(namespaces)?;
                validate_text("dynamic boundary reason", reason)
            }
            Self::Precedence { terms } => {
                if terms.is_empty() {
                    return Err("precedence relation must declare at least one dimension".into());
                }
                if terms
                    .iter()
                    .map(|term| term.dimension)
                    .collect::<BTreeSet<_>>()
                    .len()
                    != terms.len()
                {
                    return Err("precedence relation contains duplicate dimensions".into());
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionRuleSection {
    kind: ResolutionRuleSectionKind,
    support: CapabilitySupport,
    authority: Option<CapabilityAuthority>,
    instructions: Vec<ResolutionInstruction>,
}

impl ResolutionRuleSection {
    pub fn provided(
        kind: ResolutionRuleSectionKind,
        instructions: Vec<ResolutionInstruction>,
    ) -> Result<Self, String> {
        let section = Self {
            kind,
            support: CapabilitySupport::Provided,
            authority: Some(CapabilityAuthority::Adapter),
            instructions,
        };
        section.validate()?;
        Ok(section)
    }

    pub const fn unsupported(kind: ResolutionRuleSectionKind) -> Self {
        Self {
            kind,
            support: CapabilitySupport::Unsupported,
            authority: None,
            instructions: Vec::new(),
        }
    }

    pub const fn unknown(kind: ResolutionRuleSectionKind) -> Self {
        Self {
            kind,
            support: CapabilitySupport::Unknown,
            authority: None,
            instructions: Vec::new(),
        }
    }

    pub fn kind(&self) -> ResolutionRuleSectionKind {
        self.kind
    }

    pub fn support(&self) -> CapabilitySupport {
        self.support
    }

    pub fn authority(&self) -> Option<CapabilityAuthority> {
        self.authority
    }

    pub fn instructions(&self) -> &[ResolutionInstruction] {
        &self.instructions
    }

    fn validate(&self) -> Result<(), String> {
        match (self.support, self.authority, self.instructions.is_empty()) {
            (CapabilitySupport::Provided, Some(CapabilityAuthority::Adapter), false) => {}
            (CapabilitySupport::Provided, _, true) => {
                return Err(format!(
                    "provided {} rule section has no instructions",
                    self.kind.as_str()
                ));
            }
            (CapabilitySupport::Provided, _, false) => {
                return Err(format!(
                    "provided {} rule section lacks adapter authority",
                    self.kind.as_str()
                ));
            }
            (CapabilitySupport::Unknown | CapabilitySupport::Unsupported, None, true) => {}
            (CapabilitySupport::Unknown | CapabilitySupport::Unsupported, _, _) => {
                return Err(format!(
                    "unavailable {} rule section carries payload or authority",
                    self.kind.as_str()
                ));
            }
        }
        for instruction in &self.instructions {
            if instruction.section() != self.kind {
                return Err(format!(
                    "{} instruction is stored in {} section",
                    instruction.section().as_str(),
                    self.kind.as_str()
                ));
            }
            instruction.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LanguageResolutionRulePack {
    schema: String,
    adapter_schema: String,
    dialects: Vec<DialectDeclaration>,
    sections: Vec<ResolutionRuleSection>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LanguageResolutionRulePackWire {
    schema: String,
    adapter_schema: String,
    dialects: Vec<DialectDeclaration>,
    sections: Vec<ResolutionRuleSection>,
}

impl<'de> Deserialize<'de> for LanguageResolutionRulePack {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = LanguageResolutionRulePackWire::deserialize(deserializer)?;
        let pack = Self {
            schema: wire.schema,
            adapter_schema: wire.adapter_schema,
            dialects: wire.dialects,
            sections: wire.sections,
        };
        pack.validate().map_err(D::Error::custom)?;
        Ok(pack)
    }
}

impl LanguageResolutionRulePack {
    pub fn new(
        adapter_schema: impl Into<String>,
        dialects: Vec<DialectDeclaration>,
        sections: Vec<ResolutionRuleSection>,
    ) -> Result<Self, String> {
        let pack = Self {
            schema: LANGUAGE_RESOLUTION_RULE_SCHEMA.to_string(),
            adapter_schema: adapter_schema.into(),
            dialects,
            sections,
        };
        pack.validate()?;
        Ok(pack)
    }

    pub fn unknown(adapter_schema: impl Into<String>) -> Self {
        Self::new(
            adapter_schema,
            Vec::new(),
            ResolutionRuleSectionKind::ALL
                .into_iter()
                .map(ResolutionRuleSection::unknown)
                .collect(),
        )
        .expect("the total unknown resolution rule pack is valid")
    }

    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn adapter_schema(&self) -> &str {
        &self.adapter_schema
    }

    pub fn dialects(&self) -> &[DialectDeclaration] {
        &self.dialects
    }

    pub fn sections(&self) -> &[ResolutionRuleSection] {
        &self.sections
    }

    pub fn section(&self, kind: ResolutionRuleSectionKind) -> &ResolutionRuleSection {
        let index = ResolutionRuleSectionKind::ALL
            .iter()
            .position(|candidate| *candidate == kind)
            .expect("the resolution rule catalog is exhaustive");
        &self.sections[index]
    }

    pub fn has_provided_rules(&self) -> bool {
        self.sections
            .iter()
            .any(|section| section.support == CapabilitySupport::Provided)
    }

    pub fn supports_dialect(&self, dialect: &str, grammar_id: &str, grammar_version: &str) -> bool {
        self.dialects.iter().any(|candidate| {
            candidate.dialect() == dialect
                && candidate.grammar_id() == grammar_id
                && candidate.grammar_version() == grammar_version
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != LANGUAGE_RESOLUTION_RULE_SCHEMA {
            return Err(format!(
                "unsupported language resolution rule schema {}",
                self.schema
            ));
        }
        validate_text("resolution rule adapter schema", &self.adapter_schema)?;
        if self.sections.len() != ResolutionRuleSectionKind::ALL.len() {
            return Err("resolution rule section catalog is incomplete".into());
        }
        for (expected, section) in ResolutionRuleSectionKind::ALL.iter().zip(&self.sections) {
            if *expected != section.kind {
                return Err(format!(
                    "resolution rule sections are out of order: expected {}",
                    expected.as_str()
                ));
            }
            section.validate()?;
        }
        let mut dialects = BTreeSet::new();
        for dialect in &self.dialects {
            for value in [
                dialect.dialect(),
                dialect.grammar_id(),
                dialect.grammar_version(),
            ] {
                validate_text("resolution rule dialect identity", value)?;
            }
            if !dialects.insert((
                dialect.dialect(),
                dialect.grammar_id(),
                dialect.grammar_version(),
            )) {
                return Err("resolution rule pack contains a duplicate dialect".into());
            }
        }
        if self.has_provided_rules() && self.dialects.is_empty() {
            return Err("provided resolution rules require an exact dialect identity".into());
        }
        self.validate_namespace_catalog()?;
        Ok(())
    }

    fn validate_namespace_catalog(&self) -> Result<(), String> {
        let namespace_section = self.section(ResolutionRuleSectionKind::Namespaces);
        let declared = namespace_section
            .instructions
            .iter()
            .filter_map(|instruction| match instruction {
                ResolutionInstruction::DeclareNamespace { namespace } => Some(namespace),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        if namespace_section.support == CapabilitySupport::Provided && declared.is_empty() {
            return Err("provided namespace rules declare no namespaces".into());
        }
        for section in &self.sections {
            for instruction in &section.instructions {
                for namespace in instruction_namespaces(instruction) {
                    if !declared.contains(namespace) {
                        return Err(format!(
                            "{} instruction references an undeclared namespace",
                            section.kind.as_str()
                        ));
                    }
                }
            }
        }
        let precedence = self.section(ResolutionRuleSectionKind::Precedence);
        if precedence.support == CapabilitySupport::Provided && precedence.instructions.len() != 1 {
            return Err("provided precedence must contain exactly one structured relation".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuiltinResolutionFamily {
    Clojure,
    Julia,
    Python,
    JavaScript,
    TypeScript,
    Rust,
}

pub(crate) fn builtin_resolution_rule_pack(
    adapter_schema: &str,
    family: BuiltinResolutionFamily,
) -> LanguageResolutionRulePack {
    use ResolutionInstruction as I;
    use ResolutionRuleSectionKind as K;

    let provided = |kind, instructions| {
        ResolutionRuleSection::provided(kind, instructions)
            .expect("built-in resolution instructions are valid")
    };
    let namespaces = builtin_namespaces(family);
    let declared_namespaces = namespaces
        .iter()
        .cloned()
        .map(|namespace| I::DeclareNamespace { namespace })
        .collect::<Vec<_>>();
    let sections = vec![
        builtin_scope_section(family),
        ResolutionRuleSection::unknown(K::Extraction),
        provided(
            K::Namespaces,
            declared_namespaces
                .into_iter()
                .chain(builtin_namespace_relations(family))
                .collect(),
        ),
        builtin_visibility_timing_section(family),
        provided(
            K::ShadowingDuplicates,
            namespaces
                .iter()
                .cloned()
                .flat_map(|namespace| {
                    [
                        I::Shadowing {
                            inner: namespace.clone(),
                            outer: namespace.clone(),
                            rule: "nearest-declared-lexical-scope".to_string(),
                        },
                        I::DuplicateDefinitions {
                            namespace,
                            rule: builtin_duplicate_rule(family),
                        },
                    ]
                })
                .collect(),
        ),
        provided(K::Qualification, builtin_qualification(family)),
        provided(
            K::ImportsExports,
            builtin_imports(family)
                .into_iter()
                .map(|rule| I::ImportTraversal { rule })
                .collect(),
        ),
        provided(
            K::ModuleMapping,
            builtin_module_inputs(family)
                .into_iter()
                .map(|prerequisite| I::RequireModuleInput { prerequisite })
                .collect(),
        ),
        provided(K::DynamicBoundaries, builtin_dynamic_boundaries(family)),
        provided(
            K::Precedence,
            vec![I::Precedence {
                terms: builtin_precedence(family),
            }],
        ),
    ];
    LanguageResolutionRulePack::new(adapter_schema, builtin_dialects(family), sections)
        .expect("the built-in resolution rule pack is valid")
}

fn builtin_dialects(family: BuiltinResolutionFamily) -> Vec<DialectDeclaration> {
    match family {
        BuiltinResolutionFamily::Clojure => vec![DialectDeclaration::new(
            "clojure",
            "tree-sitter-clojure",
            "0.1.0",
        )],
        BuiltinResolutionFamily::Julia => vec![DialectDeclaration::new(
            "julia",
            "tree-sitter-julia",
            "0.23.1",
        )],
        BuiltinResolutionFamily::Python => vec![DialectDeclaration::new(
            "python",
            "tree-sitter-python",
            "0.25.0",
        )],
        BuiltinResolutionFamily::JavaScript => vec![
            DialectDeclaration::new("javascript", "tree-sitter-javascript", "0.25.0"),
            DialectDeclaration::new("jsx", "tree-sitter-javascript", "0.25.0"),
        ],
        BuiltinResolutionFamily::TypeScript => vec![
            DialectDeclaration::new("typescript", "tree-sitter-typescript/typescript", "0.23.2"),
            DialectDeclaration::new("tsx", "tree-sitter-typescript/tsx", "0.23.2"),
        ],
        BuiltinResolutionFamily::Rust => vec![DialectDeclaration::new(
            "rust",
            "tree-sitter-rust",
            "0.24.2",
        )],
    }
}

fn builtin_namespaces(family: BuiltinResolutionFamily) -> Vec<RuleNamespace> {
    use RuleNamespace as N;
    match family {
        BuiltinResolutionFamily::Clojure => vec![N::Value, N::Module, N::Macro, N::Member],
        BuiltinResolutionFamily::Julia => vec![N::Value, N::Type, N::Module, N::Macro, N::Member],
        BuiltinResolutionFamily::Python => vec![N::Value, N::Type, N::Module, N::Member],
        BuiltinResolutionFamily::JavaScript => vec![N::Value, N::Module, N::Label, N::Member],
        BuiltinResolutionFamily::TypeScript => {
            vec![N::Value, N::Type, N::Module, N::Label, N::Member]
        }
        BuiltinResolutionFamily::Rust => {
            vec![N::Value, N::Type, N::Module, N::Macro, N::Label, N::Member]
        }
    }
}

fn builtin_namespace_relations(
    family: BuiltinResolutionFamily,
) -> impl Iterator<Item = ResolutionInstruction> {
    use ResolutionInstruction as I;
    use RuleNamespace as N;
    let instructions = match family {
        BuiltinResolutionFamily::Clojure => vec![I::UnifyNamespaces {
            namespaces: vec![N::Value, N::Macro],
        }],
        BuiltinResolutionFamily::Julia | BuiltinResolutionFamily::Python => {
            vec![I::UnifyNamespaces {
                namespaces: vec![N::Value, N::Type, N::Module],
            }]
        }
        BuiltinResolutionFamily::JavaScript => vec![I::UnifyNamespaces {
            namespaces: vec![N::Value, N::Module],
        }],
        BuiltinResolutionFamily::TypeScript => vec![I::AllowNamespaceTransition {
            from: N::Type,
            to: N::Value,
            rule: "dual-space-declaration-only".to_string(),
        }],
        BuiltinResolutionFamily::Rust => vec![I::AllowNamespaceTransition {
            from: N::Module,
            to: N::Type,
            rule: "module-path-segment".to_string(),
        }],
    };
    instructions.into_iter()
}

fn builtin_scope_section(family: BuiltinResolutionFamily) -> ResolutionRuleSection {
    use ResolutionInstruction as I;
    use ResolutionRuleSectionKind as K;
    let rule = |raw_kind, scope_kind, parent| I::CreateScope {
        selector: ResolutionSyntaxSelector::new(raw_kind, None, None)
            .expect("built-in raw kind is valid"),
        scope_kind,
        parent,
    };
    let instructions = match family {
        BuiltinResolutionFamily::Clojure => return ResolutionRuleSection::unknown(K::ScopeModel),
        BuiltinResolutionFamily::Julia => vec![
            rule(
                "source_file",
                RuleScopeKind::File,
                ScopeParentRule::BuildContext,
            ),
            rule(
                "module_definition",
                RuleScopeKind::Module,
                ScopeParentRule::NearestDeclaredScope,
            ),
            rule(
                "function_definition",
                RuleScopeKind::Callable,
                ScopeParentRule::NearestDeclaredScope,
            ),
            rule(
                "struct_definition",
                RuleScopeKind::Type,
                ScopeParentRule::NearestDeclaredScope,
            ),
            rule(
                "let_statement",
                RuleScopeKind::Block,
                ScopeParentRule::NearestDeclaredScope,
            ),
        ],
        BuiltinResolutionFamily::Python => vec![
            rule("module", RuleScopeKind::File, ScopeParentRule::BuildContext),
            rule(
                "function_definition",
                RuleScopeKind::Callable,
                ScopeParentRule::NearestDeclaredScope,
            ),
            rule(
                "class_definition",
                RuleScopeKind::Type,
                ScopeParentRule::NearestDeclaredScope,
            ),
            rule(
                "list_comprehension",
                RuleScopeKind::Comprehension,
                ScopeParentRule::NearestDeclaredScope,
            ),
        ],
        BuiltinResolutionFamily::JavaScript | BuiltinResolutionFamily::TypeScript => vec![
            rule(
                "program",
                RuleScopeKind::File,
                ScopeParentRule::BuildContext,
            ),
            rule(
                "statement_block",
                RuleScopeKind::Block,
                ScopeParentRule::NearestDeclaredScope,
            ),
            rule(
                "function_declaration",
                RuleScopeKind::Callable,
                ScopeParentRule::NearestDeclaredScope,
            ),
            rule(
                "arrow_function",
                RuleScopeKind::Callable,
                ScopeParentRule::NearestDeclaredScope,
            ),
            rule(
                "class_declaration",
                RuleScopeKind::Type,
                ScopeParentRule::NearestDeclaredScope,
            ),
        ],
        BuiltinResolutionFamily::Rust => vec![
            rule(
                "source_file",
                RuleScopeKind::File,
                ScopeParentRule::BuildContext,
            ),
            rule(
                "mod_item",
                RuleScopeKind::Module,
                ScopeParentRule::NearestModule,
            ),
            rule(
                "function_item",
                RuleScopeKind::Callable,
                ScopeParentRule::NearestDeclaredScope,
            ),
            rule(
                "impl_item",
                RuleScopeKind::Type,
                ScopeParentRule::NearestDeclaredScope,
            ),
            rule(
                "block",
                RuleScopeKind::Block,
                ScopeParentRule::NearestDeclaredScope,
            ),
        ],
    };
    ResolutionRuleSection::provided(K::ScopeModel, instructions)
        .expect("built-in scope rules are valid")
}

fn builtin_visibility_timing_section(family: BuiltinResolutionFamily) -> ResolutionRuleSection {
    use ResolutionInstruction as I;
    use ResolutionRuleSectionKind as K;
    let timing = |kind, timing| I::DeclarationTiming {
        selector: ResolutionSyntaxSelector::new(kind, None, None)
            .expect("built-in timing selector is valid"),
        timing,
    };
    let instructions = match family {
        BuiltinResolutionFamily::Clojure
        | BuiltinResolutionFamily::JavaScript
        | BuiltinResolutionFamily::TypeScript => {
            return ResolutionRuleSection::unknown(K::VisibilityTiming);
        }
        BuiltinResolutionFamily::Julia => vec![
            timing("assignment", DeclarationTimingRule::SourceOrder),
            timing("function_definition", DeclarationTimingRule::ScopeEntry),
        ],
        BuiltinResolutionFamily::Python => vec![
            timing("assignment", DeclarationTimingRule::SourceOrder),
            timing("function_definition", DeclarationTimingRule::SourceOrder),
            timing("class_definition", DeclarationTimingRule::SourceOrder),
        ],
        BuiltinResolutionFamily::Rust => vec![
            timing("let_declaration", DeclarationTimingRule::AfterInitializer),
            timing("function_item", DeclarationTimingRule::ScopeEntry),
            timing("mod_item", DeclarationTimingRule::ScopeEntry),
        ],
    };
    ResolutionRuleSection::provided(K::VisibilityTiming, instructions)
        .expect("built-in visibility/timing rules are valid")
}

fn builtin_duplicate_rule(family: BuiltinResolutionFamily) -> DuplicateDefinitionRule {
    match family {
        BuiltinResolutionFamily::Python
        | BuiltinResolutionFamily::JavaScript
        | BuiltinResolutionFamily::Julia
        | BuiltinResolutionFamily::Clojure => DuplicateDefinitionRule::LatestVisible,
        BuiltinResolutionFamily::TypeScript => DuplicateDefinitionRule::MergeDeclarations,
        BuiltinResolutionFamily::Rust => DuplicateDefinitionRule::AdapterRejects,
    }
}

fn builtin_qualification(family: BuiltinResolutionFamily) -> Vec<ResolutionInstruction> {
    use QualificationRootRule as R;
    use ResolutionInstruction as I;
    use RuleNamespace as N;
    let member = |separator: &str| I::MemberTraversal {
        separator: separator.to_string(),
        namespace: N::Member,
    };
    match family {
        BuiltinResolutionFamily::Clojure => vec![member("/")],
        BuiltinResolutionFamily::Julia | BuiltinResolutionFamily::Python => vec![member(".")],
        BuiltinResolutionFamily::JavaScript | BuiltinResolutionFamily::TypeScript => vec![
            I::QualificationRoot {
                token: "this".into(),
                root: R::Receiver,
            },
            I::QualificationRoot {
                token: "super".into(),
                root: R::Type,
            },
            member("."),
        ],
        BuiltinResolutionFamily::Rust => vec![
            I::QualificationRoot {
                token: "self".into(),
                root: R::CurrentModule,
            },
            I::QualificationRoot {
                token: "super".into(),
                root: R::ParentModule,
            },
            I::QualificationRoot {
                token: "crate".into(),
                root: R::PackageRoot,
            },
            I::QualificationRoot {
                token: "Self".into(),
                root: R::Type,
            },
            member("::"),
            member("."),
        ],
    }
}

fn builtin_imports(family: BuiltinResolutionFamily) -> Vec<ImportTraversalRule> {
    use ImportTraversalRule as I;
    match family {
        BuiltinResolutionFamily::Clojure
        | BuiltinResolutionFamily::Julia
        | BuiltinResolutionFamily::Python => {
            vec![I::Explicit, I::Selective, I::Alias, I::Glob, I::Export]
        }
        BuiltinResolutionFamily::JavaScript | BuiltinResolutionFamily::TypeScript => vec![
            I::Explicit,
            I::Selective,
            I::Alias,
            I::Glob,
            I::Export,
            I::ReExport,
        ],
        BuiltinResolutionFamily::Rust => vec![
            I::Explicit,
            I::Selective,
            I::Alias,
            I::Glob,
            I::Prelude,
            I::Export,
        ],
    }
}

fn builtin_module_inputs(family: BuiltinResolutionFamily) -> Vec<ModulePrerequisite> {
    use ModulePrerequisite as M;
    match family {
        BuiltinResolutionFamily::Clojure => {
            vec![M::PackageManifest, M::Dependencies, M::SourceRoots]
        }
        BuiltinResolutionFamily::Julia => vec![
            M::PackageManifest,
            M::Lockfile,
            M::Dependencies,
            M::SourceRoots,
        ],
        BuiltinResolutionFamily::Python => {
            vec![M::SourceRoots, M::Dependencies, M::ModuleMap]
        }
        BuiltinResolutionFamily::JavaScript | BuiltinResolutionFamily::TypeScript => vec![
            M::PackageManifest,
            M::Lockfile,
            M::Dependencies,
            M::SourceRoots,
            M::ModuleMap,
            M::LanguageMode,
        ],
        BuiltinResolutionFamily::Rust => vec![
            M::PackageManifest,
            M::Lockfile,
            M::BuildTarget,
            M::Dependencies,
            M::SourceRoots,
            M::GeneratedRoots,
            M::Features,
            M::Platform,
            M::Prelude,
        ],
    }
}

fn builtin_dynamic_boundaries(family: BuiltinResolutionFamily) -> Vec<ResolutionInstruction> {
    use ResolutionInstruction as I;
    let namespaces = builtin_namespaces(family);
    let boundary = |raw_kind: &str, reason: &str| I::DynamicBoundary {
        selector: ResolutionSyntaxSelector::new(raw_kind, None, None)
            .expect("built-in dynamic selector is valid"),
        namespaces: namespaces.clone(),
        reason: reason.to_string(),
    };
    match family {
        BuiltinResolutionFamily::Clojure => vec![
            boundary("evaling_lit", "runtime evaluation changes visible bindings"),
            boundary(
                "read_cond_lit",
                "reader condition depends on unavailable build context",
            ),
        ],
        BuiltinResolutionFamily::Julia => vec![
            boundary("macrocall_expression", "macro expansion is not retained"),
            boundary("quote_expression", "quoted evaluation is opaque"),
        ],
        BuiltinResolutionFamily::Python => vec![
            boundary(
                "exec_statement",
                "runtime execution changes visible bindings",
            ),
            boundary(
                "call",
                "reflective import or attribute access may be dynamic",
            ),
        ],
        BuiltinResolutionFamily::JavaScript | BuiltinResolutionFamily::TypeScript => vec![
            boundary("with_statement", "with-object name lookup is dynamic"),
            boundary(
                "call_expression",
                "eval and dynamic import require exact callee rules",
            ),
        ],
        BuiltinResolutionFamily::Rust => vec![
            boundary("macro_invocation", "macro expansion is not retained"),
            boundary(
                "macro_definition",
                "macro-generated bindings are not retained",
            ),
        ],
    }
}

fn builtin_precedence(family: BuiltinResolutionFamily) -> Vec<PrecedenceTerm> {
    use PrecedenceDimension as P;
    use PrecedenceDirection::{HigherFirst, LowerFirst};
    let dimensions = match family {
        BuiltinResolutionFamily::Clojure => vec![P::RuleStep, P::Namespace, P::AdapterOrder],
        BuiltinResolutionFamily::Julia => vec![
            P::RuleStep,
            P::LexicalDistance,
            P::Namespace,
            P::ImportSpecificity,
            P::SourceOrder,
        ],
        BuiltinResolutionFamily::Python
        | BuiltinResolutionFamily::JavaScript
        | BuiltinResolutionFamily::TypeScript => vec![
            P::RuleStep,
            P::LexicalDistance,
            P::Namespace,
            P::ImportSpecificity,
            P::SourceOrder,
            P::AdapterOrder,
        ],
        BuiltinResolutionFamily::Rust => vec![
            P::RuleStep,
            P::LexicalDistance,
            P::Namespace,
            P::ImportSpecificity,
            P::AdapterOrder,
        ],
    };
    dimensions
        .into_iter()
        .map(|dimension| {
            PrecedenceTerm::new(
                dimension,
                if dimension == P::SourceOrder {
                    HigherFirst
                } else {
                    LowerFirst
                },
            )
        })
        .collect()
}

fn instruction_namespaces(instruction: &ResolutionInstruction) -> Vec<&RuleNamespace> {
    match instruction {
        ResolutionInstruction::ExtractFact {
            namespace: Some(namespace),
            ..
        }
        | ResolutionInstruction::DeclareNamespace { namespace }
        | ResolutionInstruction::DuplicateDefinitions { namespace, .. }
        | ResolutionInstruction::MemberTraversal { namespace, .. } => vec![namespace],
        ResolutionInstruction::UnifyNamespaces { namespaces }
        | ResolutionInstruction::DynamicBoundary { namespaces, .. } => namespaces.iter().collect(),
        ResolutionInstruction::AllowNamespaceTransition { from, to, .. }
        | ResolutionInstruction::Shadowing {
            inner: from,
            outer: to,
            ..
        } => vec![from, to],
        _ => Vec::new(),
    }
}

fn validate_namespaces(namespaces: &[RuleNamespace]) -> Result<(), String> {
    let mut unique = BTreeSet::new();
    for namespace in namespaces {
        namespace.validate()?;
        if !unique.insert(namespace) {
            return Err("resolution instruction contains duplicate namespaces".into());
        }
    }
    Ok(())
}

fn validate_optional_text(label: &str, value: Option<&str>) -> Result<(), String> {
    if let Some(value) = value {
        validate_text(label, value)?;
    }
    Ok(())
}

fn validate_text(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn selector(kind: &str) -> ResolutionSyntaxSelector {
        ResolutionSyntaxSelector::new(kind, None, None).unwrap()
    }

    fn section(
        kind: ResolutionRuleSectionKind,
        instructions: Vec<ResolutionInstruction>,
    ) -> ResolutionRuleSection {
        ResolutionRuleSection::provided(kind, instructions).unwrap()
    }

    fn complete_pack() -> LanguageResolutionRulePack {
        use ResolutionInstruction as I;
        use ResolutionRuleSectionKind as K;
        let sections = vec![
            section(
                K::ScopeModel,
                vec![I::CreateScope {
                    selector: selector("source_file"),
                    scope_kind: RuleScopeKind::File,
                    parent: ScopeParentRule::BuildContext,
                }],
            ),
            section(
                K::Extraction,
                vec![I::ExtractFact {
                    selector: selector("identifier"),
                    fact_kind: ExtractionFactKind::Reference,
                    name_field: None,
                    namespace: Some(RuleNamespace::Value),
                }],
            ),
            section(
                K::Namespaces,
                vec![
                    I::DeclareNamespace {
                        namespace: RuleNamespace::Value,
                    },
                    I::DeclareNamespace {
                        namespace: RuleNamespace::Member,
                    },
                ],
            ),
            section(
                K::VisibilityTiming,
                vec![I::DeclarationTiming {
                    selector: selector("identifier"),
                    timing: DeclarationTimingRule::SourceOrder,
                }],
            ),
            section(
                K::ShadowingDuplicates,
                vec![I::Shadowing {
                    inner: RuleNamespace::Value,
                    outer: RuleNamespace::Value,
                    rule: "nearest-lexical".into(),
                }],
            ),
            section(
                K::Qualification,
                vec![I::MemberTraversal {
                    separator: ".".into(),
                    namespace: RuleNamespace::Member,
                }],
            ),
            section(
                K::ImportsExports,
                vec![I::ImportTraversal {
                    rule: ImportTraversalRule::Explicit,
                }],
            ),
            section(
                K::ModuleMapping,
                vec![I::RequireModuleInput {
                    prerequisite: ModulePrerequisite::SourceRoots,
                }],
            ),
            section(
                K::DynamicBoundaries,
                vec![I::DynamicBoundary {
                    selector: selector("eval_expression"),
                    namespaces: vec![RuleNamespace::Value],
                    reason: "dynamic evaluation".into(),
                }],
            ),
            section(
                K::Precedence,
                vec![I::Precedence {
                    terms: vec![
                        PrecedenceTerm::new(
                            PrecedenceDimension::RuleStep,
                            PrecedenceDirection::LowerFirst,
                        ),
                        PrecedenceTerm::new(
                            PrecedenceDimension::LexicalDistance,
                            PrecedenceDirection::LowerFirst,
                        ),
                        PrecedenceTerm::new(
                            PrecedenceDimension::SourceOrder,
                            PrecedenceDirection::HigherFirst,
                        ),
                    ],
                }],
            ),
        ];
        LanguageResolutionRulePack::new(
            "test-adapter/1",
            vec![DialectDeclaration::new("test", "tree-sitter-test", "1.0.0")],
            sections,
        )
        .unwrap()
    }

    #[test]
    fn total_rule_pack_round_trips_strictly() {
        let pack = complete_pack();
        assert_eq!(pack.sections().len(), ResolutionRuleSectionKind::ALL.len());
        assert!(pack.sections().iter().all(|section| {
            section.support() == CapabilitySupport::Provided
                && section.authority() == Some(CapabilityAuthority::Adapter)
                && !section.instructions().is_empty()
        }));
        let json = serde_json::to_value(&pack).unwrap();
        let decoded: LanguageResolutionRulePack = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), json);

        let mut unknown = json;
        unknown["winner"] = json!("first");
        assert!(serde_json::from_value::<LanguageResolutionRulePack>(unknown).is_err());
    }

    #[test]
    fn unavailable_sections_are_payload_free_and_catalog_is_total() {
        let unknown = LanguageResolutionRulePack::unknown("test-adapter/1");
        assert!(unknown.dialects().is_empty());
        assert!(unknown.sections().iter().all(|section| {
            section.support() == CapabilitySupport::Unknown
                && section.authority().is_none()
                && section.instructions().is_empty()
        }));

        let mut json = serde_json::to_value(unknown).unwrap();
        json["sections"][0]["instructions"] = json!([{
            "operation": "precedence",
            "terms": [{"dimension": "rule-step", "direction": "lower-first"}]
        }]);
        assert!(serde_json::from_value::<LanguageResolutionRulePack>(json).is_err());
    }

    #[test]
    fn wrong_sections_namespaces_and_precedence_are_rejected() {
        assert!(
            ResolutionRuleSection::provided(
                ResolutionRuleSectionKind::ScopeModel,
                vec![ResolutionInstruction::Precedence {
                    terms: vec![PrecedenceTerm::new(
                        PrecedenceDimension::RuleStep,
                        PrecedenceDirection::LowerFirst,
                    )],
                }],
            )
            .is_err()
        );
        assert!(
            ResolutionRuleSection::provided(
                ResolutionRuleSectionKind::Precedence,
                vec![ResolutionInstruction::Precedence {
                    terms: vec![
                        PrecedenceTerm::new(
                            PrecedenceDimension::RuleStep,
                            PrecedenceDirection::LowerFirst,
                        ),
                        PrecedenceTerm::new(
                            PrecedenceDimension::RuleStep,
                            PrecedenceDirection::HigherFirst,
                        ),
                    ],
                }],
            )
            .is_err()
        );

        let mut json = serde_json::to_value(complete_pack()).unwrap();
        json["sections"][1]["instructions"][0]["namespace"] = json!("type");
        assert!(serde_json::from_value::<LanguageResolutionRulePack>(json).is_err());
    }

    #[test]
    fn built_in_rule_matrix_is_exact_total_and_honestly_partial() {
        use BuiltinResolutionFamily as F;
        use ResolutionRuleSectionKind as K;
        let cases = [
            (F::Clojure, 1, 7, 4),
            (F::Julia, 1, 9, 5),
            (F::Python, 1, 9, 4),
            (F::JavaScript, 2, 8, 4),
            (F::TypeScript, 2, 8, 5),
            (F::Rust, 1, 9, 6),
        ];
        let mut serializations = BTreeSet::new();
        for (family, dialects, provided, namespaces) in cases {
            let pack = builtin_resolution_rule_pack("deslop-lang-adapter/2", family);
            pack.validate().unwrap();
            assert_eq!(pack.schema(), LANGUAGE_RESOLUTION_RULE_SCHEMA);
            assert_eq!(pack.dialects().len(), dialects);
            assert_eq!(
                pack.sections()
                    .iter()
                    .filter(|section| section.support() == CapabilitySupport::Provided)
                    .count(),
                provided
            );
            assert_eq!(
                pack.section(K::Extraction).support(),
                CapabilitySupport::Unknown
            );
            assert!(pack.section(K::Extraction).instructions().is_empty());
            assert_eq!(
                pack.section(K::Namespaces)
                    .instructions()
                    .iter()
                    .filter(|instruction| matches!(
                        instruction,
                        ResolutionInstruction::DeclareNamespace { .. }
                    ))
                    .count(),
                namespaces
            );
            assert_eq!(pack.section(K::Precedence).instructions().len(), 1);
            serializations.insert(serde_json::to_string(&pack).unwrap());
        }
        assert_eq!(serializations.len(), cases.len());
    }
}
