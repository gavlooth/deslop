use std::fmt;
use std::path::{Path, PathBuf};

use deslop_lang::{RegionClass, RegionSpan, Registry, TailPositionClass};

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
        let pack = Registry::default().pack_for_lang(file.grammar().lang());
        Ok(nodes
            .into_iter()
            .zip(ids)
            .map(|(tree_node, node)| SyntaxAdapterFacts {
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
            })
            .collect::<Vec<_>>()
            .into_boxed_slice())
    }
}
