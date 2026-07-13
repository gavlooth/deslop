use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use deslop_lang::{
    AdapterCapability, CanonicalRoleSet, CapabilitySupport, RegionClass, RegionSpan,
    TailPositionClass,
};
use tree_sitter::Node;

use crate::arena::tree_nodes_preorder;
use crate::{NodeId, ProjectAnalysis, ProjectionId};

pub const CANONICAL_ROLE_PROJECTION_SCHEMA: &str = "deslop.canonical-role-projection/1";

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
        AdapterCapability, CanonicalRole, CapabilityAuthority, CapabilityDeclaration, GENERIC_PACK,
        GrammarDescriptor, LangPack, RUST_PACK, Registry,
    };

    use crate::{ProjectSnapshotBuilder, RepositoryId};

    struct SameLangPack {
        name: &'static str,
        schema: &'static str,
        extension: &'static str,
        branch: usize,
        canonical_roles: bool,
        manifest_adapter_schema: Option<&'static str>,
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
        manifest_adapter_schema: None,
    };
    static RIGHT_PACK: SameLangPack = SameLangPack {
        name: "same-lang-right",
        schema: "same-lang-right/11",
        extension: "right",
        branch: 11,
        canonical_roles: false,
        manifest_adapter_schema: None,
    };
    static ALTERNATE_LEFT_PACK: SameLangPack = SameLangPack {
        name: "same-lang-left-alternate",
        schema: "same-lang-left/8",
        extension: "left",
        branch: 8,
        canonical_roles: false,
        manifest_adapter_schema: None,
    };
    static CAPABILITY_LEFT_PACK: SameLangPack = SameLangPack {
        name: "same-lang-left",
        schema: "same-lang-left/7",
        extension: "left",
        branch: 7,
        canonical_roles: true,
        manifest_adapter_schema: None,
    };
    static MISMATCHED_CAPABILITY_PACK: SameLangPack = SameLangPack {
        name: "same-lang-left",
        schema: "same-lang-left/7",
        extension: "left",
        branch: 7,
        canonical_roles: false,
        manifest_adapter_schema: Some("wrong-adapter/1"),
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
}
