use std::fmt;
use std::path::{Path, PathBuf};

use deslop_lang::{RegionClass, RegionSpan, TailPositionClass};
use tree_sitter::Node;

use crate::arena::tree_nodes_preorder;
use crate::{NodeId, ProjectAnalysis};

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

impl ProjectAnalysis {
    /// Evaluate language-pack hooks once over the retained private Tree and return owned facts.
    ///
    /// Borrowed Tree-sitter nodes remain inside this call. Results are aligned with
    /// [`Self::file_node_ids`] and reference only analysis-owned [`NodeId`] values.
    pub fn syntax_adapter_facts(
        &self,
        path: &Path,
    ) -> Result<Box<[SyntaxAdapterFacts]>, SyntaxAdapterFactsError> {
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
        let mut facts = Vec::with_capacity(nodes.len());
        for (index, (tree_node, node)) in nodes.into_iter().zip(ids).enumerate() {
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
            facts.push(SyntaxAdapterFacts {
                node,
                region_class: pack.region_class(tree_node, text),
                enclosing_region: pack.enclosing_region(tree_node, text),
                long_method_region: pack.is_long_method_region(tree_node, text),
                behavioral_container: pack.is_behavioral_container(tree_node, text),
                constant_definition_region: pack.is_constant_definition_region(tree_node, text),
                duplication_data_region: pack.is_duplication_data_region(tree_node, text),
                tail_position_class: pack.tail_position_class(tree_node, text),
                metric_branch_contribution: pack.metric_branch_contribution(tree_node, text),
                metric_nesting: pack.is_metric_nesting(tree_node, text),
                metric_flow_break: pack.is_metric_flow_break(tree_node, text),
            });
        }
        Ok(facts.into_boxed_slice())
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
    use deslop_lang::{GENERIC_PACK, GrammarDescriptor, LangPack, RUST_PACK, Registry};

    use crate::{ProjectSnapshotBuilder, RepositoryId};

    struct SameLangPack {
        name: &'static str,
        schema: &'static str,
        extension: &'static str,
        branch: usize,
    }

    impl LangPack for SameLangPack {
        fn name(&self) -> &'static str {
            self.name
        }

        fn adapter_schema(&self) -> &'static str {
            self.schema
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
    };
    static RIGHT_PACK: SameLangPack = SameLangPack {
        name: "same-lang-right",
        schema: "same-lang-right/11",
        extension: "right",
        branch: 11,
    };
    static ALTERNATE_LEFT_PACK: SameLangPack = SameLangPack {
        name: "same-lang-left-alternate",
        schema: "same-lang-left/8",
        extension: "left",
        branch: 8,
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
}
