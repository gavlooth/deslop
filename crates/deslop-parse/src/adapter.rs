use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use deslop_lang::{
    AdapterCapability, CanonicalRoleSet, CapabilityAuthority, CapabilitySupport, ConstructHandling,
    ConstructPolicyKind, LanguageConstructPolicy, LanguageLexicalPolicy, LexicalClassification,
    ParseRecoveryHandling, RegionClass, RegionSpan, TailPositionClass,
};
use tree_sitter::Node;

use crate::arena::tree_nodes_preorder;
use crate::{NodeId, ProjectAnalysis, ProjectionId};

pub const CANONICAL_ROLE_PROJECTION_SCHEMA: &str = "deslop.canonical-role-projection/1";
pub const LEXICAL_TOKEN_PROJECTION_SCHEMA: &str = "deslop.lexical-token-projection/1";
pub const CONSTRUCT_POLICY_PROJECTION_SCHEMA: &str = "deslop.construct-policy-projection/1";

/// Raw grammar evidence retained alongside a canonical role set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawSyntaxFact {
    raw_kind: Box<str>,
    raw_kind_id: u16,
    raw_grammar_kind: Box<str>,
    raw_grammar_kind_id: u16,
    field: Option<Box<str>>,
}

impl RawSyntaxFact {
    pub fn raw_kind(&self) -> &str {
        &self.raw_kind
    }

    pub fn raw_kind_id(&self) -> u16 {
        self.raw_kind_id
    }

    pub fn raw_grammar_kind(&self) -> &str {
        &self.raw_grammar_kind
    }

    pub fn raw_grammar_kind_id(&self) -> u16 {
        self.raw_grammar_kind_id
    }

    pub fn field(&self) -> Option<&str> {
        self.field.as_deref()
    }
}

/// One canonical-role fact tied to an exact raw syntax node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalNodeRoles {
    node: NodeId,
    raw: RawSyntaxFact,
    roles: CanonicalRoleSet,
}

impl CanonicalNodeRoles {
    pub fn node(&self) -> NodeId {
        self.node
    }

    pub fn raw(&self) -> &RawSyntaxFact {
        &self.raw
    }

    pub fn roles(&self) -> CanonicalRoleSet {
        self.roles
    }
}

/// An owned role projection whose `NodeId` values remain valid through the retained analysis.
#[derive(Debug, Clone)]
pub struct CanonicalRoleProjection {
    id: ProjectionId,
    analysis: Arc<ProjectAnalysis>,
    path: PathBuf,
    facts: Box<[CanonicalNodeRoles]>,
}

impl CanonicalRoleProjection {
    pub fn schema(&self) -> &'static str {
        CANONICAL_ROLE_PROJECTION_SCHEMA
    }

    pub fn id(&self) -> &ProjectionId {
        &self.id
    }

    pub fn analysis(&self) -> &Arc<ProjectAnalysis> {
        &self.analysis
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn facts(&self) -> &[CanonicalNodeRoles] {
        &self.facts
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalRoleProjectionError {
    Syntax(SyntaxAdapterFactsError),
    CapabilityUnavailable {
        path: PathBuf,
        support: CapabilitySupport,
    },
    Identity {
        detail: String,
    },
}

impl fmt::Display for CanonicalRoleProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(error) => error.fmt(formatter),
            Self::CapabilityUnavailable { path, support } => write!(
                formatter,
                "canonical roles are {} for {}",
                support.as_str(),
                path.display()
            ),
            Self::Identity { detail } => {
                write!(
                    formatter,
                    "canonical role projection identity failed: {detail}"
                )
            }
        }
    }
}

impl std::error::Error for CanonicalRoleProjectionError {}

impl From<SyntaxAdapterFactsError> for CanonicalRoleProjectionError {
    fn from(error: SyntaxAdapterFactsError) -> Self {
        Self::Syntax(error)
    }
}

/// One positive-width raw grammar leaf classified by the exact stored lexical policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexicalTokenFact {
    node: NodeId,
    raw: RawSyntaxFact,
    text: Box<str>,
    classification: LexicalClassification,
}

impl LexicalTokenFact {
    pub fn node(&self) -> NodeId {
        self.node
    }

    pub fn raw(&self) -> &RawSyntaxFact {
        &self.raw
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn classification(&self) -> &LexicalClassification {
        &self.classification
    }
}

#[derive(Debug, Clone)]
pub struct LexicalTokenProjection {
    id: ProjectionId,
    analysis: Arc<ProjectAnalysis>,
    path: PathBuf,
    policy: LanguageLexicalPolicy,
    facts: Box<[LexicalTokenFact]>,
}

impl LexicalTokenProjection {
    pub fn schema(&self) -> &'static str {
        LEXICAL_TOKEN_PROJECTION_SCHEMA
    }

    pub fn id(&self) -> &ProjectionId {
        &self.id
    }

    pub fn analysis(&self) -> &Arc<ProjectAnalysis> {
        &self.analysis
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn policy(&self) -> &LanguageLexicalPolicy {
        &self.policy
    }

    pub fn facts(&self) -> &[LexicalTokenFact] {
        &self.facts
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LexicalTokenProjectionError {
    Syntax(SyntaxAdapterFactsError),
    PolicyUnavailable {
        path: PathBuf,
        support: CapabilitySupport,
    },
    Identity(String),
}

impl fmt::Display for LexicalTokenProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(error) => error.fmt(formatter),
            Self::PolicyUnavailable { path, support } => write!(
                formatter,
                "lexical classification is {} for {}",
                support.as_str(),
                path.display()
            ),
            Self::Identity(detail) => {
                write!(
                    formatter,
                    "lexical token projection identity failed: {detail}"
                )
            }
        }
    }
}

impl std::error::Error for LexicalTokenProjectionError {}

impl From<SyntaxAdapterFactsError> for LexicalTokenProjectionError {
    fn from(error: SyntaxAdapterFactsError) -> Self {
        Self::Syntax(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConstructPolicyFactKind {
    ParseError,
    MissingSyntax,
    UnsupportedConstruct,
    Macro,
    GeneratedCode,
}

impl ConstructPolicyFactKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ParseError => "parse-error",
            Self::MissingSyntax => "missing-syntax",
            Self::UnsupportedConstruct => "unsupported-construct",
            Self::Macro => "macro",
            Self::GeneratedCode => "generated-code",
        }
    }
}

impl From<ConstructPolicyKind> for ConstructPolicyFactKind {
    fn from(kind: ConstructPolicyKind) -> Self {
        match kind {
            ConstructPolicyKind::UnsupportedConstruct => Self::UnsupportedConstruct,
            ConstructPolicyKind::Macro => Self::Macro,
            ConstructPolicyKind::GeneratedCode => Self::GeneratedCode,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstructPolicyFact {
    node: NodeId,
    raw: RawSyntaxFact,
    text: Box<str>,
    kind: ConstructPolicyFactKind,
    authority: CapabilityAuthority,
    parse_handling: Option<ParseRecoveryHandling>,
    construct_handling: Option<ConstructHandling>,
}

impl ConstructPolicyFact {
    pub fn node(&self) -> NodeId {
        self.node
    }

    pub fn raw(&self) -> &RawSyntaxFact {
        &self.raw
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn kind(&self) -> ConstructPolicyFactKind {
        self.kind
    }

    pub fn authority(&self) -> CapabilityAuthority {
        self.authority
    }

    pub fn parse_handling(&self) -> Option<ParseRecoveryHandling> {
        self.parse_handling
    }

    pub fn construct_handling(&self) -> Option<ConstructHandling> {
        self.construct_handling
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialectPolicyFact {
    dialect: Box<str>,
    grammar_id: Box<str>,
    grammar_version: Box<str>,
    support: CapabilitySupport,
    authority: Option<CapabilityAuthority>,
}

impl DialectPolicyFact {
    pub fn dialect(&self) -> &str {
        &self.dialect
    }

    pub fn grammar_id(&self) -> &str {
        &self.grammar_id
    }

    pub fn grammar_version(&self) -> &str {
        &self.grammar_version
    }

    pub fn support(&self) -> CapabilitySupport {
        self.support
    }

    pub fn authority(&self) -> Option<CapabilityAuthority> {
        self.authority
    }
}

#[derive(Debug, Clone)]
pub struct ConstructPolicyProjection {
    id: ProjectionId,
    analysis: Arc<ProjectAnalysis>,
    path: PathBuf,
    policy: LanguageConstructPolicy,
    dialect: DialectPolicyFact,
    facts: Box<[ConstructPolicyFact]>,
}

impl ConstructPolicyProjection {
    pub fn schema(&self) -> &'static str {
        CONSTRUCT_POLICY_PROJECTION_SCHEMA
    }

    pub fn id(&self) -> &ProjectionId {
        &self.id
    }

    pub fn analysis(&self) -> &Arc<ProjectAnalysis> {
        &self.analysis
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn policy(&self) -> &LanguageConstructPolicy {
        &self.policy
    }

    pub fn dialect(&self) -> &DialectPolicyFact {
        &self.dialect
    }

    pub fn facts(&self) -> &[ConstructPolicyFact] {
        &self.facts
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstructPolicyProjectionError {
    Syntax(SyntaxAdapterFactsError),
    DialectMismatch {
        path: PathBuf,
        dialect: String,
        grammar_id: String,
        grammar_version: String,
    },
    Identity(String),
}

impl fmt::Display for ConstructPolicyProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(error) => error.fmt(formatter),
            Self::DialectMismatch {
                path,
                dialect,
                grammar_id,
                grammar_version,
            } => write!(
                formatter,
                "construct policy does not declare stored dialect {dialect}/{grammar_id}/{grammar_version} for {}",
                path.display()
            ),
            Self::Identity(detail) => {
                write!(
                    formatter,
                    "construct policy projection identity failed: {detail}"
                )
            }
        }
    }
}

impl std::error::Error for ConstructPolicyProjectionError {}

impl From<SyntaxAdapterFactsError> for ConstructPolicyProjectionError {
    fn from(error: SyntaxAdapterFactsError) -> Self {
        Self::Syntax(error)
    }
}

/// Owned results of language-pack syntax hooks for one existing analysis node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxAdapterFacts {
    node: NodeId,
    region_class: RegionClass,
    enclosing_region: Option<RegionSpan>,
    long_method_region: bool,
    behavioral_container: bool,
    constant_definition_region: bool,
    duplication_data_region: bool,
    tail_position_class: TailPositionClass,
    metric_branch_contribution: usize,
    metric_nesting: bool,
    metric_flow_break: bool,
}

impl SyntaxAdapterFacts {
    pub fn node(&self) -> NodeId {
        self.node
    }

    pub fn region_class(&self) -> RegionClass {
        self.region_class
    }

    pub fn enclosing_region(&self) -> Option<RegionSpan> {
        self.enclosing_region
    }

    pub fn is_long_method_region(&self) -> bool {
        self.long_method_region
    }

    pub fn is_behavioral_container(&self) -> bool {
        self.behavioral_container
    }

    pub fn is_constant_definition_region(&self) -> bool {
        self.constant_definition_region
    }

    pub fn is_duplication_data_region(&self) -> bool {
        self.duplication_data_region
    }

    pub fn tail_position_class(&self) -> TailPositionClass {
        self.tail_position_class
    }

    pub fn metric_branch_contribution(&self) -> usize {
        self.metric_branch_contribution
    }

    pub fn is_metric_nesting(&self) -> bool {
        self.metric_nesting
    }

    pub fn is_metric_flow_break(&self) -> bool {
        self.metric_flow_break
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxAdapterFactsError {
    FileNotFound {
        path: PathBuf,
    },
    SyntaxUnavailable {
        path: PathBuf,
    },
    TreeArenaMismatch {
        path: PathBuf,
        tree_nodes: usize,
        arena_nodes: usize,
    },
    TreeArenaNodeMismatch {
        path: PathBuf,
        index: usize,
        detail: String,
    },
}

impl fmt::Display for SyntaxAdapterFactsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileNotFound { path } => {
                write!(formatter, "analysis has no source file {}", path.display())
            }
            Self::SyntaxUnavailable { path } => {
                write!(formatter, "syntax is unavailable for {}", path.display())
            }
            Self::TreeArenaMismatch {
                path,
                tree_nodes,
                arena_nodes,
            } => write!(
                formatter,
                "private Tree and owned arena disagree for {}: {tree_nodes} versus {arena_nodes} nodes",
                path.display()
            ),
            Self::TreeArenaNodeMismatch {
                path,
                index,
                detail,
            } => write!(
                formatter,
                "private Tree and owned arena disagree for {} at preorder node {index}: {detail}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for SyntaxAdapterFactsError {}

struct ValidatedSyntaxNodes<'analysis> {
    pack: &'static dyn deslop_lang::LangPack,
    text: &'analysis str,
    nodes: Vec<Node<'analysis>>,
    ids: Vec<NodeId>,
}

impl ProjectAnalysis {
    /// Evaluate language-pack hooks once over the retained private Tree and return owned facts.
    ///
    /// Borrowed Tree-sitter nodes remain inside this call. Results are aligned with
    /// [`Self::file_node_ids`] and reference only analysis-owned [`NodeId`] values.
    pub fn syntax_adapter_facts(
        &self,
        path: &Path,
    ) -> Result<Box<[SyntaxAdapterFacts]>, SyntaxAdapterFactsError> {
        let syntax = self.validated_syntax_nodes(path)?;
        let mut facts = Vec::with_capacity(syntax.nodes.len());
        for (tree_node, node) in syntax.nodes.into_iter().zip(syntax.ids) {
            facts.push(SyntaxAdapterFacts {
                node,
                region_class: syntax.pack.region_class(tree_node, syntax.text),
                enclosing_region: syntax.pack.enclosing_region(tree_node, syntax.text),
                long_method_region: syntax.pack.is_long_method_region(tree_node, syntax.text),
                behavioral_container: syntax.pack.is_behavioral_container(tree_node, syntax.text),
                constant_definition_region: syntax
                    .pack
                    .is_constant_definition_region(tree_node, syntax.text),
                duplication_data_region: syntax
                    .pack
                    .is_duplication_data_region(tree_node, syntax.text),
                tail_position_class: syntax.pack.tail_position_class(tree_node, syntax.text),
                metric_branch_contribution: syntax
                    .pack
                    .metric_branch_contribution(tree_node, syntax.text),
                metric_nesting: syntax.pack.is_metric_nesting(tree_node, syntax.text),
                metric_flow_break: syntax.pack.is_metric_flow_break(tree_node, syntax.text),
            });
        }
        Ok(facts.into_boxed_slice())
    }

    /// Build the canonical-role projection declared by the exact stored language adapter.
    ///
    /// Unknown or unsupported capability is a typed failure, not an empty authoritative mapping.
    /// Each fact copies the raw grammar identity and parent field from the immutable arena, while
    /// the projection retains this analysis so its process-local node IDs cannot outlive their owner.
    pub fn canonical_role_projection(
        self: &Arc<Self>,
        path: &Path,
    ) -> Result<CanonicalRoleProjection, CanonicalRoleProjectionError> {
        let syntax = self.validated_syntax_nodes(path)?;
        let identity = self
            .snapshot()
            .entry(path)
            .and_then(|entry| entry.language_adapter_identity())
            .expect("validated source syntax has a stored adapter identity");
        let support = identity
            .capabilities()
            .declaration(AdapterCapability::CanonicalRoles)
            .support();
        if support != CapabilitySupport::Provided {
            return Err(CanonicalRoleProjectionError::CapabilityUnavailable {
                path: path.to_path_buf(),
                support,
            });
        }

        let id = self
            .derive_projection_id(
                CANONICAL_ROLE_PROJECTION_SCHEMA,
                deslop_lang::CANONICAL_ROLE_SCHEMA.as_bytes(),
                AdapterCapability::CanonicalRoles.as_str().as_bytes(),
            )
            .map_err(|error| CanonicalRoleProjectionError::Identity {
                detail: error.to_string(),
            })?;
        let mut facts = Vec::with_capacity(syntax.nodes.len());
        for (tree_node, node) in syntax.nodes.into_iter().zip(syntax.ids) {
            let view = self
                .node(node)
                .expect("validated syntax nodes belong to this analysis");
            facts.push(CanonicalNodeRoles {
                node,
                raw: RawSyntaxFact {
                    raw_kind: view.raw_kind().into(),
                    raw_kind_id: view.raw_kind_id(),
                    raw_grammar_kind: view.raw_grammar_kind().into(),
                    raw_grammar_kind_id: view.raw_grammar_kind_id(),
                    field: view.field().map(Into::into),
                },
                roles: syntax.pack.canonical_roles(tree_node, syntax.text),
            });
        }
        Ok(CanonicalRoleProjection {
            id,
            analysis: Arc::clone(self),
            path: path.to_path_buf(),
            facts: facts.into_boxed_slice(),
        })
    }

    /// Classify non-overlapping, positive-width Tree-sitter token owners with the stored policy.
    ///
    /// An explicitly classified composite node owns its complete span and suppresses its
    /// descendants; every other composite is traversed down to its leaves. Direct-child gaps and
    /// root-external bytes remain trivia ownership and are deliberately not invented as tokens.
    /// The returned projection retains this analysis and never reparses.
    pub fn lexical_token_projection(
        self: &Arc<Self>,
        path: &Path,
    ) -> Result<LexicalTokenProjection, LexicalTokenProjectionError> {
        let syntax = self.validated_syntax_nodes(path)?;
        let policy = self
            .snapshot()
            .entry(path)
            .and_then(|entry| entry.language_adapter_identity())
            .expect("validated source syntax has a stored adapter identity")
            .lexical_policy()
            .clone();
        if policy.support() != CapabilitySupport::Provided {
            return Err(LexicalTokenProjectionError::PolicyUnavailable {
                path: path.to_path_buf(),
                support: policy.support(),
            });
        }

        let id = self
            .derive_projection_id(
                LEXICAL_TOKEN_PROJECTION_SCHEMA,
                deslop_lang::LANGUAGE_LEXICAL_POLICY_SCHEMA.as_bytes(),
                b"lexical-token-classification",
            )
            .map_err(|error| LexicalTokenProjectionError::Identity(error.to_string()))?;
        let mut facts = Vec::new();
        let mut claimed_end = None;
        for (tree_node, node) in syntax.nodes.into_iter().zip(syntax.ids) {
            if tree_node.start_byte() == tree_node.end_byte() {
                continue;
            }
            if let Some(end) = claimed_end {
                if tree_node.start_byte() < end {
                    continue;
                }
                claimed_end = None;
            }
            let text = syntax
                .text
                .get(tree_node.start_byte()..tree_node.end_byte())
                .expect("validated syntax spans are UTF-8 boundaries");
            let classification = if tree_node.child_count() == 0 {
                policy
                    .classify(tree_node.kind(), text)
                    .expect("a validated provided lexical policy has a terminal fallback")
            } else if let Some(classification) = policy.classify_explicit(tree_node.kind(), text) {
                claimed_end = Some(tree_node.end_byte());
                classification
            } else {
                continue;
            }
            .clone();
            let view = self
                .node(node)
                .expect("validated syntax nodes belong to this analysis");
            facts.push(LexicalTokenFact {
                node,
                raw: RawSyntaxFact {
                    raw_kind: view.raw_kind().into(),
                    raw_kind_id: view.raw_kind_id(),
                    raw_grammar_kind: view.raw_grammar_kind().into(),
                    raw_grammar_kind_id: view.raw_grammar_kind_id(),
                    field: view.field().map(Into::into),
                },
                text: text.into(),
                classification,
            });
        }
        Ok(LexicalTokenProjection {
            id,
            analysis: Arc::clone(self),
            path: path.to_path_buf(),
            policy,
            facts: facts.into_boxed_slice(),
        })
    }

    /// Project exact parse-recovery, construct-boundary, and stored-dialect policy facts.
    ///
    /// Error and missing facts come from retained grammar flags. Other facts require an exact
    /// adapter rule. The returned projection retains this analysis and never reparses.
    pub fn construct_policy_projection(
        self: &Arc<Self>,
        path: &Path,
    ) -> Result<ConstructPolicyProjection, ConstructPolicyProjectionError> {
        let syntax = self.validated_syntax_nodes(path)?;
        let entry = self
            .snapshot()
            .entry(path)
            .expect("validated source syntax has a stored snapshot entry");
        let identity = entry
            .language_adapter_identity()
            .expect("validated source syntax has a stored adapter identity");
        let policy = identity.construct_policy().clone();
        let grammar = entry
            .grammar()
            .expect("validated source syntax has a stored grammar identity");
        if policy.dialects().support() == CapabilitySupport::Provided
            && policy
                .dialects()
                .declaration(
                    grammar.dialect(),
                    grammar.grammar_id(),
                    grammar.grammar_version(),
                )
                .is_none()
        {
            return Err(ConstructPolicyProjectionError::DialectMismatch {
                path: path.to_path_buf(),
                dialect: grammar.dialect().to_string(),
                grammar_id: grammar.grammar_id().to_string(),
                grammar_version: grammar.grammar_version().to_string(),
            });
        }

        let id = self
            .derive_projection_id(
                CONSTRUCT_POLICY_PROJECTION_SCHEMA,
                deslop_lang::LANGUAGE_CONSTRUCT_POLICY_SCHEMA.as_bytes(),
                b"construct-recovery-dialect-policy",
            )
            .map_err(|error| ConstructPolicyProjectionError::Identity(error.to_string()))?;
        let mut facts = Vec::new();
        for (tree_node, node) in syntax.nodes.into_iter().zip(syntax.ids) {
            let text = syntax
                .text
                .get(tree_node.start_byte()..tree_node.end_byte())
                .expect("validated syntax spans are UTF-8 boundaries");
            let view = self
                .node(node)
                .expect("validated syntax nodes belong to this analysis");
            let raw = || RawSyntaxFact {
                raw_kind: view.raw_kind().into(),
                raw_kind_id: view.raw_kind_id(),
                raw_grammar_kind: view.raw_grammar_kind().into(),
                raw_grammar_kind_id: view.raw_grammar_kind_id(),
                field: view.field().map(Into::into),
            };

            let recovery = policy.parse_recovery();
            if recovery.support() == CapabilitySupport::Provided {
                let kind = if view.is_error() {
                    Some(ConstructPolicyFactKind::ParseError)
                } else if view.is_missing() {
                    Some(ConstructPolicyFactKind::MissingSyntax)
                } else {
                    None
                };
                if let Some(kind) = kind {
                    facts.push(ConstructPolicyFact {
                        node,
                        raw: raw(),
                        text: text.into(),
                        kind,
                        authority: recovery
                            .authority()
                            .expect("validated provided recovery policy has authority"),
                        parse_handling: recovery.handling(),
                        construct_handling: None,
                    });
                }
            }

            for section in policy.constructs() {
                if let Some(rule) = section.matching_rule(tree_node.kind(), text) {
                    facts.push(ConstructPolicyFact {
                        node,
                        raw: raw(),
                        text: text.into(),
                        kind: section.kind().into(),
                        authority: section
                            .authority()
                            .expect("validated provided construct section has authority"),
                        parse_handling: None,
                        construct_handling: Some(rule.handling()),
                    });
                }
            }
        }

        Ok(ConstructPolicyProjection {
            id,
            analysis: Arc::clone(self),
            path: path.to_path_buf(),
            dialect: DialectPolicyFact {
                dialect: grammar.dialect().into(),
                grammar_id: grammar.grammar_id().into(),
                grammar_version: grammar.grammar_version().into(),
                support: policy.dialects().support(),
                authority: policy.dialects().authority(),
            },
            policy,
            facts: facts.into_boxed_slice(),
        })
    }

    fn validated_syntax_nodes<'analysis>(
        &'analysis self,
        path: &Path,
    ) -> Result<ValidatedSyntaxNodes<'analysis>, SyntaxAdapterFactsError> {
        let file = self
            .file(path)
            .ok_or_else(|| SyntaxAdapterFactsError::FileNotFound {
                path: path.to_path_buf(),
            })?;
        let tree = file
            .query_tree()
            .ok_or_else(|| SyntaxAdapterFactsError::SyntaxUnavailable {
                path: path.to_path_buf(),
            })?;
        let text = file
            .text()
            .ok_or_else(|| SyntaxAdapterFactsError::SyntaxUnavailable {
                path: path.to_path_buf(),
            })?;
        let nodes = tree_nodes_preorder(tree);
        let ids = self
            .file_node_ids(path)
            .expect("an analysis file always owns a node range")
            .collect::<Vec<_>>();
        if nodes.len() != ids.len() {
            return Err(SyntaxAdapterFactsError::TreeArenaMismatch {
                path: path.to_path_buf(),
                tree_nodes: nodes.len(),
                arena_nodes: ids.len(),
            });
        }
        let pack = self.language_adapter(path).ok_or_else(|| {
            SyntaxAdapterFactsError::SyntaxUnavailable {
                path: path.to_path_buf(),
            }
        })?;
        for (index, (&tree_node, &node)) in nodes.iter().zip(&ids).enumerate() {
            let view = self.node(node).map_err(|error| {
                SyntaxAdapterFactsError::TreeArenaNodeMismatch {
                    path: path.to_path_buf(),
                    index,
                    detail: error.to_string(),
                }
            })?;
            let span = view.span();
            let tree_field = tree_node_field(tree_node);
            if tree_node.kind() != view.raw_kind()
                || tree_node.kind_id() != view.raw_kind_id()
                || tree_node.grammar_name() != view.raw_grammar_kind()
                || tree_node.grammar_id() != view.raw_grammar_kind_id()
                || tree_node.start_byte() != span.start_byte()
                || tree_node.end_byte() != span.end_byte()
                || tree_node.start_position().row != span.start_point().row()
                || tree_node.start_position().column != span.start_point().column()
                || tree_node.end_position().row != span.end_point().row()
                || tree_node.end_position().column != span.end_point().column()
                || tree_field.as_deref() != view.field()
                || tree_node.is_named() != view.is_named()
                || tree_node.is_extra() != view.is_extra()
                || tree_node.is_error() != view.is_error()
                || tree_node.is_missing() != view.is_missing()
                || tree_node.has_error() != view.has_error()
            {
                return Err(SyntaxAdapterFactsError::TreeArenaNodeMismatch {
                    path: path.to_path_buf(),
                    index,
                    detail: format!(
                        "Tree node {} {:?} does not match arena node {} {:?}",
                        tree_node.kind(),
                        tree_node.byte_range(),
                        view.raw_kind(),
                        span.start_byte()..span.end_byte()
                    ),
                });
            }
        }
        Ok(ValidatedSyntaxNodes {
            pack,
            text,
            nodes,
            ids,
        })
    }
}

fn tree_node_field(node: Node<'_>) -> Option<String> {
    let parent = node.parent()?;
    let mut cursor = parent.walk();
    parent
        .children(&mut cursor)
        .enumerate()
        .find(|(_, child)| child.id() == node.id())
        .and_then(|(index, _)| parent.field_name_for_child(index as u32))
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use deslop_core::Lang;
    use deslop_lang::{
        AdapterCapability, CanonicalRole, CapabilityAuthority, CapabilityDeclaration,
        ConstructHandling, ConstructPolicyKind, ConstructPolicySection, ConstructRule,
        DialectDeclaration, DialectPolicy, GENERIC_PACK, GrammarDescriptor, IdentifierCasePolicy,
        LangPack, LanguageConstructPolicy, LanguageLexicalPolicy, LanguageQueryPack,
        LexicalClassification, LexicalOperatorClass, LexicalRule, LexicalTokenClass,
        ParseRecoveryHandling, ParseRecoveryPolicy, QueryCaptureDeclaration, QueryFamily,
        QueryFamilyDeclaration, RUST_PACK, Registry,
    };

    use crate::{ProjectSnapshotBuilder, RepositoryId};

    struct SameLangPack {
        name: &'static str,
        schema: &'static str,
        extension: &'static str,
        branch: usize,
        canonical_roles: bool,
        queries: bool,
        query_capture_mismatch: bool,
        lexical: bool,
        constructs: bool,
        construct_dialect_mismatch: bool,
        manifest_adapter_schema: Option<&'static str>,
        query_adapter_schema: Option<&'static str>,
        lexical_adapter_schema: Option<&'static str>,
        construct_adapter_schema: Option<&'static str>,
    }

    impl LangPack for SameLangPack {
        fn name(&self) -> &'static str {
            self.name
        }

        fn adapter_schema(&self) -> &'static str {
            self.schema
        }

        fn capability_manifest(&self) -> deslop_lang::LanguageAdapterCapabilityManifest {
            let manifest = deslop_lang::LanguageAdapterCapabilityManifest::current_syntax(
                self.manifest_adapter_schema
                    .unwrap_or(self.adapter_schema()),
            );
            if self.canonical_roles {
                manifest
                    .with_declaration(CapabilityDeclaration::provided(
                        AdapterCapability::CanonicalRoles,
                        CapabilityAuthority::Adapter,
                    ))
                    .unwrap()
            } else {
                manifest
            }
        }

        fn canonical_roles(&self, node: Node<'_>, _text: &str) -> CanonicalRoleSet {
            if !self.canonical_roles {
                return CanonicalRoleSet::default();
            }
            CanonicalRoleSet::from_roles(match node.kind() {
                "source_file" => vec![CanonicalRole::Project],
                "type_item" => vec![CanonicalRole::Declaration, CanonicalRole::Type],
                "type_identifier" | "primitive_type" | "generic_type" => {
                    vec![CanonicalRole::Type]
                }
                "function_item" => {
                    vec![CanonicalRole::Declaration, CanonicalRole::Callable]
                }
                "parameters" | "parameter" => vec![CanonicalRole::Parameter],
                "block" => vec![CanonicalRole::Block],
                "expression_statement" => vec![CanonicalRole::Statement],
                "call_expression" => vec![CanonicalRole::Expression, CanonicalRole::Call],
                "identifier" => vec![CanonicalRole::Expression, CanonicalRole::Read],
                "integer_literal" | "string_literal" => {
                    vec![CanonicalRole::Expression, CanonicalRole::Literal]
                }
                "ERROR" => vec![CanonicalRole::Error],
                _ => Vec::new(),
            })
        }

        fn query_pack(&self) -> LanguageQueryPack {
            let adapter_schema = self.query_adapter_schema.unwrap_or(self.adapter_schema());
            if !self.queries {
                return LanguageQueryPack::unknown(adapter_schema);
            }
            let capture = |name, roles: &[CanonicalRole]| {
                QueryCaptureDeclaration::new(
                    name,
                    CanonicalRoleSet::from_roles(roles.iter().copied()),
                )
                .unwrap()
            };
            LanguageQueryPack::new(
                adapter_schema,
                vec![
                    QueryFamilyDeclaration::provided(
                        QueryFamily::Declarations,
                        CapabilityAuthority::Adapter,
                        "(function_item) @declaration",
                        vec![capture(
                            if self.query_capture_mismatch {
                                "wrong-declaration"
                            } else {
                                "declaration"
                            },
                            &[CanonicalRole::Declaration, CanonicalRole::Callable],
                        )],
                    ),
                    QueryFamilyDeclaration::provided(
                        QueryFamily::References,
                        CapabilityAuthority::Adapter,
                        "(call_expression function: (identifier) @reference)",
                        vec![capture(
                            "reference",
                            &[CanonicalRole::Expression, CanonicalRole::Read],
                        )],
                    ),
                    QueryFamilyDeclaration::provided(
                        QueryFamily::Scopes,
                        CapabilityAuthority::Adapter,
                        "(block) @scope",
                        vec![capture(
                            "scope",
                            &[CanonicalRole::Module, CanonicalRole::Block],
                        )],
                    ),
                    QueryFamilyDeclaration::provided(
                        QueryFamily::Control,
                        CapabilityAuthority::Adapter,
                        "(if_expression) @control",
                        vec![capture(
                            "control",
                            &[CanonicalRole::Expression, CanonicalRole::Branch],
                        )],
                    ),
                    QueryFamilyDeclaration::provided(
                        QueryFamily::Comments,
                        CapabilityAuthority::Adapter,
                        "(line_comment) @comment",
                        vec![capture("comment", &[CanonicalRole::Comment])],
                    ),
                    QueryFamilyDeclaration::provided(
                        QueryFamily::OpaqueGenerated,
                        CapabilityAuthority::Adapter,
                        "(macro_invocation) @opaque\n(attribute_item) @generated",
                        vec![
                            capture("opaque", &[CanonicalRole::OpaqueRegion]),
                            capture("generated", &[CanonicalRole::Generated]),
                        ],
                    ),
                ],
            )
            .unwrap()
        }

        fn lexical_policy(&self) -> LanguageLexicalPolicy {
            let adapter_schema = self.lexical_adapter_schema.unwrap_or(self.adapter_schema());
            if !self.lexical {
                return LanguageLexicalPolicy::unknown(adapter_schema);
            }
            let token = |raw_kind, class| {
                LexicalRule::new(raw_kind, None, LexicalClassification::token(class))
            };
            LanguageLexicalPolicy::provided(
                adapter_schema,
                CapabilityAuthority::Adapter,
                IdentifierCasePolicy::Sensitive,
                true,
                vec!["//".to_string()],
                vec![deslop_lang::BlockCommentDelimiter::new("/*", "*/", true)],
                vec![
                    LexicalRule::new(
                        "==",
                        Some("==".to_string()),
                        LexicalClassification::operator(LexicalOperatorClass::Comparison),
                    ),
                    LexicalRule::new(
                        "=",
                        Some("=".to_string()),
                        LexicalClassification::operator(LexicalOperatorClass::Assignment),
                    ),
                    LexicalRule::new(
                        "+",
                        Some("+".to_string()),
                        LexicalClassification::operator(LexicalOperatorClass::Arithmetic),
                    ),
                    LexicalRule::new(
                        "&&",
                        Some("&&".to_string()),
                        LexicalClassification::operator(LexicalOperatorClass::Logical),
                    ),
                    token("identifier", LexicalTokenClass::Identifier),
                    token("line_comment", LexicalTokenClass::Comment),
                    token("block_comment", LexicalTokenClass::Comment),
                    token("integer_literal", LexicalTokenClass::Literal),
                    token("string_literal", LexicalTokenClass::Literal),
                    token("true", LexicalTokenClass::Literal),
                    token("false", LexicalTokenClass::Literal),
                    token("fn", LexicalTokenClass::Keyword),
                    token("let", LexicalTokenClass::Keyword),
                    token("if", LexicalTokenClass::Keyword),
                    token("(", LexicalTokenClass::Delimiter),
                    token(")", LexicalTokenClass::Delimiter),
                    token("{", LexicalTokenClass::Delimiter),
                    token("}", LexicalTokenClass::Delimiter),
                    token(";", LexicalTokenClass::Punctuation),
                    token(",", LexicalTokenClass::Punctuation),
                    token(":", LexicalTokenClass::Punctuation),
                    token("*", LexicalTokenClass::Other),
                ],
            )
            .unwrap()
        }

        fn construct_policy(&self) -> LanguageConstructPolicy {
            let adapter_schema = self
                .construct_adapter_schema
                .unwrap_or(self.adapter_schema());
            if !self.constructs {
                return LanguageConstructPolicy::unknown(adapter_schema);
            }
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
                        if self.construct_dialect_mismatch {
                            "wrong-dialect"
                        } else {
                            "same-lang"
                        },
                        "tree-sitter-rust",
                        "test",
                    )],
                )
                .unwrap(),
            )
            .unwrap()
        }

        fn lang(&self) -> Lang {
            Lang::Rust
        }

        fn extensions(&self) -> &'static [&'static str] {
            if self.extension == "left" {
                &["left"]
            } else {
                &["right"]
            }
        }

        fn grammar(&self) -> Option<tree_sitter::Language> {
            RUST_PACK.grammar()
        }

        fn grammar_descriptor_for_path(&self, _path: &Path) -> Option<GrammarDescriptor> {
            Some(GrammarDescriptor::new(
                Lang::Rust,
                "same-lang",
                "tree-sitter-rust",
                "test",
            ))
        }

        fn line_comments(&self) -> &'static [&'static str] {
            &["//"]
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

        fn metric_branch_contribution(&self, _node: Node<'_>, _text: &str) -> usize {
            self.branch
        }

        fn halstead_operator_tokens(&self) -> &'static [&'static str] {
            &[]
        }

        fn enclosing_region(
            &self,
            _node: Node<'_>,
            _text: &str,
        ) -> Option<deslop_lang::RegionSpan> {
            None
        }
    }

    static LEFT_PACK: SameLangPack = SameLangPack {
        name: "same-lang-left",
        schema: "same-lang-left/7",
        extension: "left",
        branch: 7,
        canonical_roles: false,
        queries: false,
        query_capture_mismatch: false,
        lexical: false,
        constructs: false,
        construct_dialect_mismatch: false,
        manifest_adapter_schema: None,
        query_adapter_schema: None,
        lexical_adapter_schema: None,
        construct_adapter_schema: None,
    };
    static RIGHT_PACK: SameLangPack = SameLangPack {
        name: "same-lang-right",
        schema: "same-lang-right/11",
        extension: "right",
        branch: 11,
        canonical_roles: false,
        queries: false,
        query_capture_mismatch: false,
        lexical: false,
        constructs: false,
        construct_dialect_mismatch: false,
        manifest_adapter_schema: None,
        query_adapter_schema: None,
        lexical_adapter_schema: None,
        construct_adapter_schema: None,
    };
    static ALTERNATE_LEFT_PACK: SameLangPack = SameLangPack {
        name: "same-lang-left-alternate",
        schema: "same-lang-left/8",
        extension: "left",
        branch: 8,
        canonical_roles: false,
        queries: false,
        query_capture_mismatch: false,
        lexical: false,
        constructs: false,
        construct_dialect_mismatch: false,
        manifest_adapter_schema: None,
        query_adapter_schema: None,
        lexical_adapter_schema: None,
        construct_adapter_schema: None,
    };
    static CAPABILITY_LEFT_PACK: SameLangPack = SameLangPack {
        name: "same-lang-left",
        schema: "same-lang-left/7",
        extension: "left",
        branch: 7,
        canonical_roles: true,
        queries: false,
        query_capture_mismatch: false,
        lexical: false,
        constructs: false,
        construct_dialect_mismatch: false,
        manifest_adapter_schema: None,
        query_adapter_schema: None,
        lexical_adapter_schema: None,
        construct_adapter_schema: None,
    };
    static QUERY_LEFT_PACK: SameLangPack = SameLangPack {
        name: "same-lang-left",
        schema: "same-lang-left/7",
        extension: "left",
        branch: 7,
        canonical_roles: false,
        queries: true,
        query_capture_mismatch: false,
        lexical: false,
        constructs: false,
        construct_dialect_mismatch: false,
        manifest_adapter_schema: None,
        query_adapter_schema: None,
        lexical_adapter_schema: None,
        construct_adapter_schema: None,
    };
    static LEXICAL_LEFT_PACK: SameLangPack = SameLangPack {
        name: "same-lang-left",
        schema: "same-lang-left/7",
        extension: "left",
        branch: 7,
        canonical_roles: false,
        queries: false,
        query_capture_mismatch: false,
        lexical: true,
        constructs: false,
        construct_dialect_mismatch: false,
        manifest_adapter_schema: None,
        query_adapter_schema: None,
        lexical_adapter_schema: None,
        construct_adapter_schema: None,
    };
    static CONSTRUCT_LEFT_PACK: SameLangPack = SameLangPack {
        name: "same-lang-left",
        schema: "same-lang-left/7",
        extension: "left",
        branch: 7,
        canonical_roles: false,
        queries: false,
        query_capture_mismatch: false,
        lexical: false,
        constructs: true,
        construct_dialect_mismatch: false,
        manifest_adapter_schema: None,
        query_adapter_schema: None,
        lexical_adapter_schema: None,
        construct_adapter_schema: None,
    };
    static CONSTRUCT_DIALECT_MISMATCH_PACK: SameLangPack = SameLangPack {
        name: "same-lang-left",
        schema: "same-lang-left/7",
        extension: "left",
        branch: 7,
        canonical_roles: false,
        queries: false,
        query_capture_mismatch: false,
        lexical: false,
        constructs: true,
        construct_dialect_mismatch: true,
        manifest_adapter_schema: None,
        query_adapter_schema: None,
        lexical_adapter_schema: None,
        construct_adapter_schema: None,
    };
    static BAD_QUERY_CAPTURE_PACK: SameLangPack = SameLangPack {
        name: "same-lang-left-bad-query",
        schema: "same-lang-left-bad-query/1",
        extension: "left",
        branch: 7,
        canonical_roles: false,
        queries: true,
        query_capture_mismatch: true,
        lexical: false,
        constructs: false,
        construct_dialect_mismatch: false,
        manifest_adapter_schema: None,
        query_adapter_schema: None,
        lexical_adapter_schema: None,
        construct_adapter_schema: None,
    };
    static MISMATCHED_CAPABILITY_PACK: SameLangPack = SameLangPack {
        name: "same-lang-left",
        schema: "same-lang-left/7",
        extension: "left",
        branch: 7,
        canonical_roles: false,
        queries: false,
        query_capture_mismatch: false,
        lexical: false,
        constructs: false,
        construct_dialect_mismatch: false,
        manifest_adapter_schema: Some("wrong-adapter/1"),
        query_adapter_schema: None,
        lexical_adapter_schema: None,
        construct_adapter_schema: None,
    };
    static MISMATCHED_QUERY_PACK: SameLangPack = SameLangPack {
        name: "same-lang-left",
        schema: "same-lang-left/7",
        extension: "left",
        branch: 7,
        canonical_roles: false,
        queries: false,
        query_capture_mismatch: false,
        lexical: false,
        constructs: false,
        construct_dialect_mismatch: false,
        manifest_adapter_schema: None,
        query_adapter_schema: Some("wrong-query-adapter/1"),
        lexical_adapter_schema: None,
        construct_adapter_schema: None,
    };
    static MISMATCHED_LEXICAL_PACK: SameLangPack = SameLangPack {
        name: "same-lang-left",
        schema: "same-lang-left/7",
        extension: "left",
        branch: 7,
        canonical_roles: false,
        queries: false,
        query_capture_mismatch: false,
        lexical: false,
        constructs: false,
        construct_dialect_mismatch: false,
        manifest_adapter_schema: None,
        query_adapter_schema: None,
        lexical_adapter_schema: Some("wrong-lexical-adapter/1"),
        construct_adapter_schema: None,
    };
    static MISMATCHED_CONSTRUCT_PACK: SameLangPack = SameLangPack {
        name: "same-lang-left",
        schema: "same-lang-left/7",
        extension: "left",
        branch: 7,
        canonical_roles: false,
        queries: false,
        query_capture_mismatch: false,
        lexical: false,
        constructs: false,
        construct_dialect_mismatch: false,
        manifest_adapter_schema: None,
        query_adapter_schema: None,
        lexical_adapter_schema: None,
        construct_adapter_schema: Some("wrong-construct-adapter/1"),
    };

    #[test]
    fn syntax_facts_use_the_exact_stored_pack_when_lang_values_collide() {
        let root = tempfile::tempdir().unwrap();
        let mut registry = Registry::new(&GENERIC_PACK);
        registry.register(&LEFT_PACK);
        registry.register(&RIGHT_PACK);
        let snapshot = ProjectSnapshotBuilder::new(
            root.path(),
            RepositoryId::explicit("same-lang-adapter-test").unwrap(),
        )
        .unwrap()
        .with_registry(registry)
        .with_overlay("sample.left", b"fn left() {}\n".to_vec())
        .unwrap()
        .with_overlay("sample.right", b"fn right() {}\n".to_vec())
        .unwrap()
        .build()
        .unwrap();
        let analysis = ProjectAnalysis::build(snapshot).unwrap();

        for (path, name, schema, branch) in [
            ("sample.left", "same-lang-left", "same-lang-left/7", 7),
            ("sample.right", "same-lang-right", "same-lang-right/11", 11),
        ] {
            let entry = analysis.snapshot().entry(Path::new(path)).unwrap();
            let identity = entry.language_adapter_identity().unwrap();
            assert_eq!((identity.name(), identity.schema()), (name, schema));
            let facts = analysis.syntax_adapter_facts(Path::new(path)).unwrap();
            assert!(
                facts
                    .iter()
                    .all(|fact| fact.metric_branch_contribution() == branch)
            );
        }
    }

    #[test]
    fn canonical_role_projection_preserves_every_raw_kind_and_field() {
        let root = tempfile::tempdir().unwrap();
        let mut registry = Registry::new(&GENERIC_PACK);
        registry.register(&CAPABILITY_LEFT_PACK);
        let path = Path::new("roles.left");
        let snapshot = ProjectSnapshotBuilder::new(
            root.path(),
            RepositoryId::explicit("canonical-role-projection-test").unwrap(),
        )
        .unwrap()
        .with_registry(registry)
        .with_overlay(
            path,
            b"type Alias = Vec<String>;\nfn sample(value: i32) { value(); }\n".to_vec(),
        )
        .unwrap()
        .build()
        .unwrap();
        let analysis = ProjectAnalysis::build(snapshot).unwrap();
        let raw_analysis_id = analysis.id().clone();
        let projection = analysis.canonical_role_projection(path).unwrap();
        let repeated = analysis.canonical_role_projection(path).unwrap();

        assert_eq!(projection.schema(), CANONICAL_ROLE_PROJECTION_SCHEMA);
        assert_eq!(projection.path(), path);
        assert!(Arc::ptr_eq(projection.analysis(), &analysis));
        assert_eq!(projection.id(), repeated.id());
        assert_eq!(analysis.id(), &raw_analysis_id);
        assert_eq!(
            projection.id(),
            &analysis
                .derive_projection_id(
                    CANONICAL_ROLE_PROJECTION_SCHEMA,
                    deslop_lang::CANONICAL_ROLE_SCHEMA.as_bytes(),
                    AdapterCapability::CanonicalRoles.as_str().as_bytes(),
                )
                .unwrap()
        );
        assert_eq!(projection.facts(), repeated.facts());
        assert_eq!(
            projection.facts().len(),
            analysis.file_node_ids(path).unwrap().len()
        );

        for fact in projection.facts() {
            let raw = fact.raw();
            let view = analysis.node(fact.node()).unwrap();
            assert_eq!(view.key().schema(), "deslop.node-key/1");
            assert!(
                !serde_json::to_string(view.key())
                    .unwrap()
                    .contains("canonical_role")
            );
            assert_eq!(raw.raw_kind(), view.raw_kind());
            assert_eq!(raw.raw_kind_id(), view.raw_kind_id());
            assert_eq!(raw.raw_grammar_kind(), view.raw_grammar_kind());
            assert_eq!(raw.raw_grammar_kind_id(), view.raw_grammar_kind_id());
            assert_eq!(raw.field(), view.field());
        }

        let alias = projection
            .facts()
            .iter()
            .find(|fact| {
                fact.raw().raw_kind() == "type_identifier"
                    && fact.raw().raw_grammar_kind() == "identifier"
            })
            .expect("the projection must retain an aliased visible and raw grammar kind");
        assert_eq!(alias.raw().field(), Some("name"));
        assert!(alias.roles().contains(CanonicalRole::Type));

        let function = projection
            .facts()
            .iter()
            .find(|fact| fact.raw().raw_kind() == "function_item")
            .unwrap();
        assert_eq!(
            function.roles().iter().collect::<Vec<_>>(),
            [CanonicalRole::Declaration, CanonicalRole::Callable]
        );

        let node_count = projection.facts().len();
        let raw_field_count = projection
            .facts()
            .iter()
            .filter(|fact| fact.raw().field().is_some())
            .count();
        let role_assignments = projection
            .facts()
            .iter()
            .map(|fact| fact.roles().len())
            .sum::<usize>();
        assert_eq!(
            (node_count, raw_field_count, role_assignments),
            (32, 11, 22)
        );
    }

    #[test]
    fn canonical_role_projection_rejects_unknown_capability() {
        let root = tempfile::tempdir().unwrap();
        let snapshot = ProjectSnapshotBuilder::new(
            root.path(),
            RepositoryId::explicit("canonical-role-unavailable-test").unwrap(),
        )
        .unwrap()
        .with_overlay("unknown.rs", b"fn sample() {}\n".to_vec())
        .unwrap()
        .build()
        .unwrap();
        let analysis = ProjectAnalysis::build(snapshot).unwrap();
        assert_eq!(
            analysis
                .canonical_role_projection(Path::new("unknown.rs"))
                .unwrap_err(),
            CanonicalRoleProjectionError::CapabilityUnavailable {
                path: PathBuf::from("unknown.rs"),
                support: CapabilitySupport::Unknown,
            }
        );
    }

    #[test]
    fn lexical_projection_classifies_non_overlapping_token_owners_without_reparse() {
        let root = tempfile::tempdir().unwrap();
        let source = b"fn sample(\xCF\x80: i32) {\n    // note\n    let value = \xCF\x80 + 1;\n    /* block */\n    if value == 2 && true {}\n}\n";
        let build = |adapter: &'static dyn LangPack| {
            let mut registry = Registry::new(&GENERIC_PACK);
            registry.register(adapter);
            let snapshot = ProjectSnapshotBuilder::new(
                root.path(),
                RepositoryId::explicit("lexical-token-projection-test").unwrap(),
            )
            .unwrap()
            .with_registry(registry)
            .with_overlay("tokens.left", source.to_vec())
            .unwrap()
            .build()
            .unwrap();
            ProjectAnalysis::build(snapshot).unwrap()
        };
        let unknown = build(&LEFT_PACK);
        let analysis = build(&LEXICAL_LEFT_PACK);
        assert_eq!(unknown.id(), analysis.id());
        let path = Path::new("tokens.left");
        assert_eq!(
            unknown.lexical_token_projection(path).unwrap_err(),
            LexicalTokenProjectionError::PolicyUnavailable {
                path: path.to_path_buf(),
                support: CapabilitySupport::Unknown,
            }
        );

        let parse_counts = analysis.parse_counts();
        let projection = analysis.lexical_token_projection(path).unwrap();
        let repeated = analysis.lexical_token_projection(path).unwrap();
        assert!(Arc::ptr_eq(projection.analysis(), &analysis));
        assert_eq!(projection.schema(), LEXICAL_TOKEN_PROJECTION_SCHEMA);
        assert_eq!(projection.id(), repeated.id());
        assert_eq!(projection.facts(), repeated.facts());
        assert_eq!(analysis.parse_counts(), parse_counts);
        assert!(
            parse_counts
                .values()
                .all(|count| count.parser_invocations == 1)
        );
        assert_ne!(
            unknown
                .derive_projection_id(
                    LEXICAL_TOKEN_PROJECTION_SCHEMA,
                    deslop_lang::LANGUAGE_LEXICAL_POLICY_SCHEMA.as_bytes(),
                    b"lexical-token-classification",
                )
                .unwrap(),
            projection.id().clone()
        );

        for fact in projection.facts() {
            let view = analysis.node(fact.node()).unwrap();
            assert!(!fact.text().is_empty());
            assert_eq!(fact.text(), view.text());
            assert_eq!(fact.raw().raw_kind(), view.raw_kind());
            assert!(!fact.text().chars().all(char::is_whitespace));
        }
        for pair in projection.facts().windows(2) {
            let left = analysis.node(pair[0].node()).unwrap().span().byte_range();
            let right = analysis.node(pair[1].node()).unwrap().span().byte_range();
            assert!(left.end <= right.start, "token-owner spans overlap");
        }
        let unicode = projection
            .facts()
            .iter()
            .find(|fact| fact.text() == "π")
            .unwrap();
        assert_eq!(
            unicode.classification().token_class(),
            LexicalTokenClass::Identifier
        );
        let equality = projection
            .facts()
            .iter()
            .find(|fact| fact.text() == "==")
            .unwrap();
        assert_eq!(
            equality.classification().operator_class(),
            Some(LexicalOperatorClass::Comparison)
        );
        assert!(projection.facts().iter().any(|fact| {
            fact.text() == "// note"
                && fact.classification().token_class() == LexicalTokenClass::Comment
        }));
        assert!(projection.facts().iter().any(|fact| {
            fact.text() == "/* block */"
                && fact.classification().token_class() == LexicalTokenClass::Comment
        }));

        let mut classes = std::collections::BTreeMap::new();
        for fact in projection.facts() {
            *classes
                .entry(fact.classification().token_class().as_str())
                .or_insert(0_usize) += 1;
        }
        assert_eq!(projection.facts().len(), 26);
        assert_eq!(
            classes,
            std::collections::BTreeMap::from([
                ("comment", 2),
                ("delimiter", 6),
                ("identifier", 5),
                ("keyword", 3),
                ("literal", 3),
                ("operator", 4),
                ("other", 1),
                ("punctuation", 2),
            ])
        );
        let mut operators = std::collections::BTreeMap::new();
        for fact in projection.facts() {
            if let Some(class) = fact.classification().operator_class() {
                *operators.entry(class.as_str()).or_insert(0_usize) += 1;
            }
        }
        assert_eq!(
            operators,
            std::collections::BTreeMap::from([
                ("arithmetic", 1),
                ("assignment", 1),
                ("comparison", 1),
                ("logical", 1),
            ])
        );

        let identity = analysis
            .snapshot()
            .entry(path)
            .unwrap()
            .language_adapter_identity()
            .unwrap();
        assert_eq!(identity.lexical_policy(), projection.policy());
        let mut legacy = serde_json::to_value(identity).unwrap();
        legacy.as_object_mut().unwrap().remove("lexical");
        assert!(
            serde_json::from_value::<crate::LanguageAdapterIdentity>(legacy)
                .unwrap_err()
                .to_string()
                .contains("missing field `lexical`")
        );
    }

    #[test]
    fn construct_projection_retains_recovery_boundaries_and_exact_dialect_without_reparse() {
        let root = tempfile::tempdir().unwrap();
        let source = b"#[generated]\nfn sample() { unsafe { vec![1]; } let broken = ; }\n";
        let build = |adapter: &'static dyn LangPack| {
            let mut registry = Registry::new(&GENERIC_PACK);
            registry.register(adapter);
            let snapshot = ProjectSnapshotBuilder::new(
                root.path(),
                RepositoryId::explicit("construct-policy-projection-test").unwrap(),
            )
            .unwrap()
            .with_registry(registry)
            .with_overlay("constructs.left", source.to_vec())
            .unwrap()
            .build()
            .unwrap();
            ProjectAnalysis::build(snapshot).unwrap()
        };
        let unknown = build(&LEFT_PACK);
        let analysis = build(&CONSTRUCT_LEFT_PACK);
        assert_eq!(unknown.id(), analysis.id());
        let path = Path::new("constructs.left");

        let unknown_projection = unknown.construct_policy_projection(path).unwrap();
        assert_eq!(
            unknown_projection.dialect().support(),
            CapabilitySupport::Unknown
        );
        assert!(unknown_projection.facts().is_empty());

        let parse_counts = analysis.parse_counts();
        let projection = analysis.construct_policy_projection(path).unwrap();
        let repeated = analysis.construct_policy_projection(path).unwrap();
        assert!(Arc::ptr_eq(projection.analysis(), &analysis));
        assert_eq!(projection.schema(), CONSTRUCT_POLICY_PROJECTION_SCHEMA);
        assert_eq!(projection.id(), repeated.id());
        assert_eq!(projection.facts(), repeated.facts());
        assert_eq!(analysis.parse_counts(), parse_counts);
        assert!(
            parse_counts
                .values()
                .all(|count| count.parser_invocations == 1)
        );
        assert_ne!(unknown_projection.id(), projection.id());
        assert_eq!(projection.dialect().dialect(), "same-lang");
        assert_eq!(projection.dialect().grammar_id(), "tree-sitter-rust");
        assert_eq!(projection.dialect().grammar_version(), "test");
        assert_eq!(projection.dialect().support(), CapabilitySupport::Provided);
        assert_eq!(
            projection.dialect().authority(),
            Some(CapabilityAuthority::Syntax)
        );

        for fact in projection.facts() {
            let view = analysis.node(fact.node()).unwrap();
            assert_eq!(fact.text(), view.text());
            assert_eq!(fact.raw().raw_kind(), view.raw_kind());
        }
        assert_eq!(projection.facts().len(), 4);
        assert_eq!(
            projection
                .facts()
                .iter()
                .map(|fact| (fact.kind().as_str(), fact.raw().raw_kind(), fact.text()))
                .collect::<Vec<_>>(),
            [
                ("generated-code", "attribute_item", "#[generated]"),
                (
                    "unsupported-construct",
                    "unsafe_block",
                    "unsafe { vec![1]; }"
                ),
                ("macro", "macro_invocation", "vec![1]"),
                ("parse-error", "ERROR", "="),
            ]
        );
        for fact in &projection.facts()[..3] {
            assert_eq!(fact.authority(), CapabilityAuthority::Adapter);
            assert_eq!(fact.parse_handling(), None);
        }
        assert_eq!(
            projection.facts()[0].construct_handling(),
            Some(ConstructHandling::SurfaceSyntax)
        );
        for fact in &projection.facts()[1..3] {
            assert_eq!(fact.construct_handling(), Some(ConstructHandling::Opaque));
        }
        let recovery = &projection.facts()[3];
        assert_eq!(recovery.authority(), CapabilityAuthority::Syntax);
        assert_eq!(
            recovery.parse_handling(),
            Some(ParseRecoveryHandling::FileIncomplete)
        );
        assert_eq!(recovery.construct_handling(), None);

        let mismatch = build(&CONSTRUCT_DIALECT_MISMATCH_PACK);
        assert_eq!(mismatch.id(), analysis.id());
        assert!(matches!(
            mismatch.construct_policy_projection(path).unwrap_err(),
            ConstructPolicyProjectionError::DialectMismatch {
                dialect,
                grammar_id,
                grammar_version,
                ..
            } if dialect == "same-lang"
                && grammar_id == "tree-sitter-rust"
                && grammar_version == "test"
        ));

        let identity = analysis
            .snapshot()
            .entry(path)
            .unwrap()
            .language_adapter_identity()
            .unwrap();
        assert_eq!(identity.construct_policy(), projection.policy());
        let mut legacy = serde_json::to_value(identity).unwrap();
        legacy.as_object_mut().unwrap().remove("constructs");
        assert!(
            serde_json::from_value::<crate::LanguageAdapterIdentity>(legacy)
                .unwrap_err()
                .to_string()
                .contains("missing field `constructs`")
        );
    }

    #[test]
    fn stored_query_pack_compiles_and_executes_all_six_families() {
        let root = tempfile::tempdir().unwrap();
        let source = b"#[generated]\nfn sample(value: i32) {\n    // note\n    if value > 0 { helper(); }\n    vec![value];\n}\n";
        let build = |adapter: &'static dyn LangPack| {
            let mut registry = Registry::new(&GENERIC_PACK);
            registry.register(adapter);
            let snapshot = ProjectSnapshotBuilder::new(
                root.path(),
                RepositoryId::explicit("language-query-pack-test").unwrap(),
            )
            .unwrap()
            .with_registry(registry)
            .with_overlay("queries.left", source.to_vec())
            .unwrap()
            .build()
            .unwrap();
            ProjectAnalysis::build(snapshot).unwrap()
        };
        let unknown = build(&LEFT_PACK);
        let provided = build(&QUERY_LEFT_PACK);
        assert_eq!(unknown.id(), provided.id());

        let path = Path::new("queries.left");
        let unavailable = unknown.compile_language_query_pack(path).unwrap();
        let parse_counts = provided.parse_counts();
        let projection = provided.compile_language_query_pack(path).unwrap();
        assert!(Arc::ptr_eq(projection.analysis(), &provided));
        assert_eq!(projection.schema(), crate::LANGUAGE_QUERY_PROJECTION_SCHEMA);
        assert_eq!(projection.path(), path);
        assert_eq!(unavailable.compiled().len(), 0);
        assert_eq!(projection.compiled().len(), 6);
        assert_ne!(unavailable.id(), projection.id());
        assert!(
            unavailable
                .pack()
                .queries()
                .iter()
                .all(|query| query.support() == CapabilitySupport::Unknown)
        );
        assert!(
            projection
                .pack()
                .queries()
                .iter()
                .all(|query| query.support() == CapabilitySupport::Provided)
        );

        let root_node = provided.file_node_ids(path).unwrap().next().unwrap();
        let capture_counts = QueryFamily::ALL.map(|family| {
            let compiled = projection.query(family).unwrap();
            assert_eq!(
                compiled.query().capture_names().collect::<Vec<_>>(),
                compiled
                    .declaration()
                    .captures()
                    .iter()
                    .map(QueryCaptureDeclaration::name)
                    .collect::<Vec<_>>()
            );
            provided
                .syntax_query_matches(compiled.query(), root_node)
                .unwrap()
                .iter()
                .map(|query_match| query_match.captures().len())
                .sum::<usize>()
        });
        assert_eq!(capture_counts, [1, 1, 2, 1, 1, 2]);
        assert_eq!(capture_counts.into_iter().sum::<usize>(), 8);
        assert_eq!(provided.parse_counts(), parse_counts);
        assert!(
            parse_counts
                .values()
                .all(|count| count.parser_invocations == 1)
        );

        let identity = provided
            .snapshot()
            .entry(path)
            .unwrap()
            .language_adapter_identity()
            .unwrap();
        assert_eq!(identity.queries(), projection.pack());
        let mut legacy = serde_json::to_value(identity).unwrap();
        legacy.as_object_mut().unwrap().remove("queries");
        assert!(
            serde_json::from_value::<crate::LanguageAdapterIdentity>(legacy)
                .unwrap_err()
                .to_string()
                .contains("missing field `queries`")
        );
    }

    #[test]
    fn stored_query_pack_rejects_capture_contract_drift() {
        let root = tempfile::tempdir().unwrap();
        let mut registry = Registry::new(&GENERIC_PACK);
        registry.register(&BAD_QUERY_CAPTURE_PACK);
        let snapshot = ProjectSnapshotBuilder::new(
            root.path(),
            RepositoryId::explicit("query-capture-contract-test").unwrap(),
        )
        .unwrap()
        .with_registry(registry)
        .with_overlay("bad.left", b"fn sample() {}\n".to_vec())
        .unwrap()
        .build()
        .unwrap();
        let analysis = ProjectAnalysis::build(snapshot).unwrap();
        assert_eq!(
            analysis
                .compile_language_query_pack(Path::new("bad.left"))
                .unwrap_err(),
            crate::LanguageQueryProjectionError::CaptureContractMismatch {
                family: QueryFamily::Declarations,
                declared: vec!["wrong-declaration".to_string()],
                compiled: vec!["declaration".to_string()],
            }
        );
    }

    #[test]
    fn projection_identity_changes_when_only_the_stored_adapter_identity_changes() {
        let root = tempfile::tempdir().unwrap();
        let build = |adapter: &'static dyn LangPack| {
            let mut registry = Registry::new(&GENERIC_PACK);
            registry.register(adapter);
            let snapshot = ProjectSnapshotBuilder::new(
                root.path(),
                RepositoryId::explicit("adapter-projection-identity-test").unwrap(),
            )
            .unwrap()
            .with_registry(registry)
            .with_overlay("sample.left", b"fn sample() {}\n".to_vec())
            .unwrap()
            .build()
            .unwrap();
            ProjectAnalysis::build(snapshot).unwrap()
        };
        let first = build(&LEFT_PACK);
        let alternate = build(&ALTERNATE_LEFT_PACK);
        assert_eq!(first.id(), alternate.id());
        assert_ne!(
            first
                .derive_projection_id("test-projection/1", b"policy", b"capability")
                .unwrap(),
            alternate
                .derive_projection_id("test-projection/1", b"policy", b"capability")
                .unwrap()
        );
    }

    #[test]
    fn projection_identity_changes_when_only_adapter_capabilities_change() {
        let root = tempfile::tempdir().unwrap();
        let build = |adapter: &'static dyn LangPack| {
            let mut registry = Registry::new(&GENERIC_PACK);
            registry.register(adapter);
            let snapshot = ProjectSnapshotBuilder::new(
                root.path(),
                RepositoryId::explicit("adapter-capability-identity-test").unwrap(),
            )
            .unwrap()
            .with_registry(registry)
            .with_overlay("sample.left", b"fn sample() {}\n".to_vec())
            .unwrap()
            .build()
            .unwrap();
            ProjectAnalysis::build(snapshot).unwrap()
        };
        let unknown = build(&LEFT_PACK);
        let provided = build(&CAPABILITY_LEFT_PACK);
        assert_eq!(unknown.id(), provided.id());
        let unknown_identity = unknown
            .snapshot()
            .entry(Path::new("sample.left"))
            .unwrap()
            .language_adapter_identity()
            .unwrap();
        let provided_identity = provided
            .snapshot()
            .entry(Path::new("sample.left"))
            .unwrap()
            .language_adapter_identity()
            .unwrap();
        assert_eq!(unknown_identity.name(), provided_identity.name());
        assert_eq!(unknown_identity.schema(), provided_identity.schema());
        assert_eq!(
            unknown_identity.capabilities().highest_complete_tier(),
            None
        );
        assert_eq!(
            provided_identity.capabilities().highest_complete_tier(),
            Some(deslop_lang::SemanticTier::S1)
        );
        assert_ne!(
            unknown
                .derive_projection_id("test-projection/1", b"policy", b"capability")
                .unwrap(),
            provided
                .derive_projection_id("test-projection/1", b"policy", b"capability")
                .unwrap()
        );

        let mut legacy = serde_json::to_value(unknown_identity).unwrap();
        legacy.as_object_mut().unwrap().remove("capabilities");
        assert!(
            serde_json::from_value::<crate::LanguageAdapterIdentity>(legacy)
                .unwrap_err()
                .to_string()
                .contains("missing field `capabilities`")
        );
    }

    #[test]
    fn snapshot_rejects_capability_manifest_for_another_adapter_schema() {
        let root = tempfile::tempdir().unwrap();
        let mut registry = Registry::new(&GENERIC_PACK);
        registry.register(&MISMATCHED_CAPABILITY_PACK);
        let error = ProjectSnapshotBuilder::new(
            root.path(),
            RepositoryId::explicit("adapter-capability-mismatch-test").unwrap(),
        )
        .unwrap()
        .with_registry(registry)
        .with_overlay("sample.left", b"fn sample() {}\n".to_vec())
        .unwrap()
        .build()
        .unwrap_err();
        assert!(
            error.to_string().contains(
                "capability manifest targets wrong-adapter/1 but adapter schema is same-lang-left/7"
            ),
            "{error}"
        );
    }

    #[test]
    fn snapshot_rejects_query_pack_for_another_adapter_schema() {
        let root = tempfile::tempdir().unwrap();
        let mut registry = Registry::new(&GENERIC_PACK);
        registry.register(&MISMATCHED_QUERY_PACK);
        let error = ProjectSnapshotBuilder::new(
            root.path(),
            RepositoryId::explicit("adapter-query-mismatch-test").unwrap(),
        )
        .unwrap()
        .with_registry(registry)
        .with_overlay("sample.left", b"fn sample() {}\n".to_vec())
        .unwrap()
        .build()
        .unwrap_err();
        assert!(
            error.to_string().contains(
                "query pack targets wrong-query-adapter/1 but adapter schema is same-lang-left/7"
            ),
            "{error}"
        );
    }

    #[test]
    fn snapshot_rejects_lexical_policy_for_another_adapter_schema() {
        let root = tempfile::tempdir().unwrap();
        let mut registry = Registry::new(&GENERIC_PACK);
        registry.register(&MISMATCHED_LEXICAL_PACK);
        let error = ProjectSnapshotBuilder::new(
            root.path(),
            RepositoryId::explicit("adapter-lexical-mismatch-test").unwrap(),
        )
        .unwrap()
        .with_registry(registry)
        .with_overlay("sample.left", b"fn sample() {}\n".to_vec())
        .unwrap()
        .build()
        .unwrap_err();
        assert!(
            error.to_string().contains(
                "lexical policy targets wrong-lexical-adapter/1 but adapter schema is same-lang-left/7"
            ),
            "{error}"
        );
    }

    #[test]
    fn snapshot_rejects_construct_policy_for_another_adapter_schema() {
        let root = tempfile::tempdir().unwrap();
        let mut registry = Registry::new(&GENERIC_PACK);
        registry.register(&MISMATCHED_CONSTRUCT_PACK);
        let error = ProjectSnapshotBuilder::new(
            root.path(),
            RepositoryId::explicit("construct-policy-adapter-schema-mismatch-test").unwrap(),
        )
        .unwrap()
        .with_registry(registry)
        .with_overlay("mismatch.left", b"fn sample() {}\n".to_vec())
        .unwrap()
        .build()
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("construct policy targets wrong-construct-adapter/1")
        );
    }
}
