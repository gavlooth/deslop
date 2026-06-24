use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use deslop_core::{Lang, Span};
use deslop_lang::Registry;
use deslop_parse::{SourceFile, parse_tree};

use crate::extract::{extract_source, signature_for_node, span_for_node};
use crate::ids::{external_id, file_id, import_keys, module_keys, simple_name, symbol_id};
use crate::types::{
    DependencyGraph, GraphConfidence, GraphConfig, GraphEdge, GraphEdgeKind, GraphNode,
    GraphNodeKind, GraphNotice, GraphSummary, Owner, PendingEdge, SCHEMA, SymbolDef,
};

#[derive(Debug)]
pub(crate) struct GraphBuilder {
    config: GraphConfig,
    nodes: BTreeMap<String, GraphNode>,
    edges: BTreeMap<EdgeKey, GraphEdge>,
    pending: Vec<PendingEdge>,
    local_symbols: BTreeMap<String, Vec<String>>,
    qualified_symbols: BTreeMap<String, String>,
    files_by_module_key: BTreeMap<String, String>,
    notices: Vec<GraphNotice>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct EdgeKey {
    kind: GraphEdgeKind,
    from: String,
    to: String,
    label: Option<String>,
    start_byte: Option<usize>,
}

impl GraphBuilder {
    pub(crate) fn new(config: GraphConfig) -> Self {
        Self {
            config,
            nodes: BTreeMap::new(),
            edges: BTreeMap::new(),
            pending: Vec::new(),
            local_symbols: BTreeMap::new(),
            qualified_symbols: BTreeMap::new(),
            files_by_module_key: BTreeMap::new(),
            notices: Vec::new(),
        }
    }

    pub(crate) fn include_calls(&self) -> bool {
        self.config.include_calls
    }

    pub(crate) fn index_file_path(&mut self, path: &Path, registry: &Registry) {
        let pack = registry.pack_for_path(path);
        let file_id = file_id(path);
        for key in module_keys(path, pack.lang()) {
            self.files_by_module_key
                .entry(key)
                .or_insert(file_id.clone());
        }
    }

    pub(crate) fn add_source(&mut self, source: &SourceFile, registry: &Registry) -> Result<()> {
        let pack = registry.pack_for_lang(source.lang);
        let file_id = self.add_file_node(source);
        let Some(tree) = parse_tree(source.lang, &source.text)? else {
            self.notices.push(GraphNotice {
                path: source.path.clone(),
                message: format!("{} has no tree-sitter grammar", pack.name()),
            });
            return Ok(());
        };
        if tree.root_node().has_error() {
            self.notices.push(GraphNotice {
                path: source.path.clone(),
                message: "tree-sitter reported ERROR nodes; graph extraction skipped for this file"
                    .to_string(),
            });
            return Ok(());
        }

        extract_source(self, source, tree.root_node(), file_id);
        Ok(())
    }

    fn add_file_node(&mut self, source: &SourceFile) -> String {
        let id = file_id(&source.path);
        self.nodes.entry(id.clone()).or_insert_with(|| GraphNode {
            id: id.clone(),
            kind: GraphNodeKind::File,
            lang: source.lang,
            path: Some(source.path.clone()),
            name: source.path.display().to_string(),
            qualified_name: source.path.display().to_string(),
            span: Some(Span::new(
                1,
                source.lines().len().max(1),
                0,
                source.text.len(),
            )),
            signature: None,
        });
        id
    }

    pub(crate) fn add_symbol_node(
        &mut self,
        source: &SourceFile,
        owner: &Owner,
        node: tree_sitter::Node<'_>,
        def: SymbolDef,
    ) -> Owner {
        let span = span_for_node(source, node);
        let id = symbol_id(&source.path, def.kind, &def.name, span.start_byte);
        let qualified_name = qualified_name(owner, &def.name);
        let signature = signature_for_node(source, node);
        self.insert_symbol_node(source, &id, &def, &qualified_name, span, signature);
        self.index_symbol(&id, &def.name, &qualified_name);
        self.add_contains_edge(owner, &id, &def.name, span);
        Owner {
            id,
            kind: def.kind,
            name: qualified_name,
        }
    }

    fn insert_symbol_node(
        &mut self,
        source: &SourceFile,
        id: &str,
        def: &SymbolDef,
        qualified_name: &str,
        span: Span,
        signature: Option<String>,
    ) {
        self.nodes
            .entry(id.to_string())
            .or_insert_with(|| GraphNode {
                id: id.to_string(),
                kind: def.kind,
                lang: source.lang,
                path: Some(source.path.clone()),
                name: def.name.clone(),
                qualified_name: qualified_name.to_string(),
                span: Some(span),
                signature,
            });
    }

    fn index_symbol(&mut self, id: &str, name: &str, qualified_name: &str) {
        let id = id.to_string();
        self.local_symbols
            .entry(simple_name(name))
            .or_default()
            .push(id.clone());
        self.qualified_symbols
            .entry(qualified_name.to_string())
            .or_insert(id.clone());
        self.qualified_symbols.entry(name.to_string()).or_insert(id);
    }

    fn add_contains_edge(&mut self, owner: &Owner, id: &str, name: &str, span: Span) {
        self.add_edge(GraphEdge {
            kind: GraphEdgeKind::Contains,
            from: owner.id.clone(),
            to: id.to_string(),
            confidence: GraphConfidence::Resolved,
            label: Some(name.to_string()),
            span: Some(span),
        });
    }

    pub(crate) fn add_pending_edge(
        &mut self,
        kind: GraphEdgeKind,
        from: &Owner,
        source: &SourceFile,
        node: tree_sitter::Node<'_>,
        label: String,
    ) {
        let label = compact_label(&label);
        if label.is_empty() {
            return;
        }
        self.pending.push(PendingEdge {
            kind,
            from: from.id.clone(),
            label,
            span: span_for_node(source, node),
            path: source.path.clone(),
            lang: source.lang,
        });
    }

    fn add_edge(&mut self, edge: GraphEdge) {
        let key = EdgeKey {
            kind: edge.kind,
            from: edge.from.clone(),
            to: edge.to.clone(),
            label: edge.label.clone(),
            start_byte: edge.span.map(|span| span.start_byte),
        };
        self.edges.entry(key).or_insert(edge);
    }

    pub(crate) fn finish(mut self) -> DependencyGraph {
        self.resolve_pending_edges();
        let nodes = self.nodes.into_values().collect::<Vec<_>>();
        let edges = self.edges.into_values().collect::<Vec<_>>();
        DependencyGraph {
            schema: SCHEMA.to_string(),
            summary: graph_summary(&nodes, &edges),
            agent_notes: agent_notes(),
            nodes,
            edges,
            notices: self.notices,
        }
    }

    fn resolve_pending_edges(&mut self) {
        for pending in std::mem::take(&mut self.pending) {
            let (target, confidence) = match pending.kind {
                GraphEdgeKind::Imports => self
                    .resolve_import(&pending)
                    .map(|target| (target, GraphConfidence::Resolved))
                    .unwrap_or_else(|| {
                        (
                            self.external_node(pending.lang, &pending.label, pending.kind),
                            GraphConfidence::External,
                        )
                    }),
                GraphEdgeKind::Calls | GraphEdgeKind::Inherits => self.resolve_symbol(&pending),
                GraphEdgeKind::Contains => continue,
            };
            self.add_edge(GraphEdge {
                kind: pending.kind,
                from: pending.from,
                to: target,
                confidence,
                label: Some(pending.label),
                span: Some(pending.span),
            });
        }
    }

    fn resolve_symbol(&mut self, pending: &PendingEdge) -> (String, GraphConfidence) {
        if let Some(target) = self.qualified_symbols.get(&pending.label) {
            return (target.clone(), GraphConfidence::Resolved);
        }
        let simple = simple_name(&pending.label);
        match self.local_symbols.get(&simple).map(Vec::as_slice) {
            Some([target]) => (target.clone(), GraphConfidence::Resolved),
            Some(candidates) if candidates.len() > 1 => (
                self.external_node(pending.lang, &pending.label, pending.kind),
                GraphConfidence::Ambiguous,
            ),
            _ => (
                self.external_node(pending.lang, &pending.label, pending.kind),
                GraphConfidence::External,
            ),
        }
    }

    fn resolve_import(&self, pending: &PendingEdge) -> Option<String> {
        import_keys(&pending.path, pending.lang, &pending.label)
            .into_iter()
            .find_map(|key| self.files_by_module_key.get(&key).cloned())
    }

    fn external_node(&mut self, lang: Lang, label: &str, edge_kind: GraphEdgeKind) -> String {
        let id = external_id(lang, edge_kind, label);
        self.nodes.entry(id.clone()).or_insert_with(|| GraphNode {
            id: id.clone(),
            kind: GraphNodeKind::ExternalSymbol,
            lang,
            path: None,
            name: label.to_string(),
            qualified_name: label.to_string(),
            span: None,
            signature: None,
        });
        id
    }
}

fn graph_summary(nodes: &[GraphNode], edges: &[GraphEdge]) -> GraphSummary {
    GraphSummary {
        files: count_nodes(nodes, |node| node.kind == GraphNodeKind::File),
        symbols: count_nodes(nodes, |node| {
            !matches!(
                node.kind,
                GraphNodeKind::File | GraphNodeKind::ExternalSymbol
            )
        }),
        external_symbols: count_nodes(nodes, |node| node.kind == GraphNodeKind::ExternalSymbol),
        edges: edges.len(),
        resolved_edges: count_edges(edges, GraphConfidence::Resolved),
        ambiguous_edges: count_edges(edges, GraphConfidence::Ambiguous),
        external_edges: count_edges(edges, GraphConfidence::External),
    }
}

fn count_nodes(nodes: &[GraphNode], predicate: impl Fn(&GraphNode) -> bool) -> usize {
    let mut count = 0;
    for node in nodes {
        if predicate(node) {
            count += 1;
        }
    }
    count
}

fn count_edges(edges: &[GraphEdge], confidence: GraphConfidence) -> usize {
    edges
        .iter()
        .filter(|edge| edge.confidence == confidence)
        .count()
}

fn agent_notes() -> Vec<String> {
    [
        "Use contains edges for ownership boundaries before rewriting.",
        "Use incoming calls/imports edges to find refactor impact.",
        "Only confidence=resolved means deslop found one local target; external and ambiguous edges require verification.",
        "This graph is deterministic tree-sitter evidence, not a behavior-preservation proof; run verify/apply before writing.",
    ]
    .into_iter()
    .map(ToString::to_string)
    .collect()
}

fn qualified_name(owner: &Owner, name: &str) -> String {
    if owner.kind == GraphNodeKind::File {
        name.to_string()
    } else {
        format!("{}::{name}", owner.name)
    }
}

fn compact_label(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
