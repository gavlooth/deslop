use std::path::PathBuf;

use deslop_core::{AnalysisStatus, FileAnalysis, Lang, Span};
use serde::{Deserialize, Serialize};

pub(crate) const SCHEMA: &str = "deslop.graph/2";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphConfig {
    pub include_calls: bool,
}

impl Default for GraphConfig {
    fn default() -> Self {
        Self {
            include_calls: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyGraph {
    pub schema: String,
    pub status: AnalysisStatus,
    pub analyses: Vec<FileAnalysis>,
    pub summary: GraphSummary,
    pub agent_notes: Vec<String>,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub notices: Vec<GraphNotice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphSummary {
    pub files: usize,
    pub symbols: usize,
    pub external_symbols: usize,
    pub edges: usize,
    pub resolved_edges: usize,
    pub ambiguous_edges: usize,
    pub external_edges: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub kind: GraphNodeKind,
    pub lang: Lang,
    pub path: Option<PathBuf>,
    pub name: String,
    pub qualified_name: String,
    pub span: Option<Span>,
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GraphNodeKind {
    File,
    Module,
    Namespace,
    Function,
    Method,
    Class,
    Struct,
    Enum,
    Trait,
    Interface,
    Constant,
    Variable,
    ExternalSymbol,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub kind: GraphEdgeKind,
    pub from: String,
    pub to: String,
    pub confidence: GraphConfidence,
    pub label: Option<String>,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GraphEdgeKind {
    Contains,
    Imports,
    Calls,
    Inherits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GraphConfidence {
    Resolved,
    Syntactic,
    Ambiguous,
    External,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNotice {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingEdge {
    pub(crate) kind: GraphEdgeKind,
    pub(crate) from: String,
    pub(crate) label: String,
    pub(crate) span: Span,
    pub(crate) path: PathBuf,
    pub(crate) lang: Lang,
}

#[derive(Debug, Clone)]
pub(crate) struct Owner {
    pub(crate) id: String,
    pub(crate) kind: GraphNodeKind,
    pub(crate) name: String,
}

#[derive(Debug, Clone)]
pub(crate) struct SymbolDef {
    pub(crate) kind: GraphNodeKind,
    pub(crate) name: String,
}
