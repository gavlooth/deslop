use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Result;
use deslop_core::{Lang, Span};
use deslop_lang::Registry;
use deslop_parse::{SourceFile, parse_source};

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
    qualified_symbols: BTreeMap<String, Vec<String>>,
    symbols_by_owner_name: BTreeMap<(String, String), Vec<String>>,
    local_bindings_by_owner_name: BTreeSet<(String, String)>,
    import_bindings_by_owner_name: BTreeSet<(String, String)>,
    parent_by_symbol: BTreeMap<String, String>,
    files_by_module_key: BTreeMap<String, Vec<String>>,
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
            symbols_by_owner_name: BTreeMap::new(),
            local_bindings_by_owner_name: BTreeSet::new(),
            import_bindings_by_owner_name: BTreeSet::new(),
            parent_by_symbol: BTreeMap::new(),
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
            push_candidate(&mut self.files_by_module_key, key, &file_id);
        }
    }

    pub(crate) fn add_source(&mut self, source: &SourceFile, registry: &Registry) -> Result<()> {
        let pack = registry.pack_for_lang(source.lang);
        let file_id = self.add_file_node(source);
        let Some(tree) = parse_source(source)? else {
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
        self.index_symbol(source, owner, &id, &def.name, &qualified_name);
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

    fn index_symbol(
        &mut self,
        source: &SourceFile,
        owner: &Owner,
        id: &str,
        name: &str,
        qualified_name: &str,
    ) {
        let simple = simple_name(name);
        push_candidate(&mut self.local_symbols, simple.clone(), id);
        push_candidate(
            &mut self.symbols_by_owner_name,
            (owner.id.clone(), simple),
            id,
        );
        self.parent_by_symbol
            .insert(id.to_string(), owner.id.clone());

        push_candidate(
            &mut self.qualified_symbols,
            normalize_qualified_label(qualified_name),
            id,
        );
        let file_prefix = format!("{}::", source.path.display());
        if let Some(relative) = qualified_name.strip_prefix(&file_prefix)
            && relative.contains("::")
        {
            push_candidate(
                &mut self.qualified_symbols,
                normalize_qualified_label(relative),
                id,
            );
        }
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

    pub(crate) fn add_binding(&mut self, owner: &Owner, name: String, is_import: bool) {
        let name = simple_name(&name);
        if !name.is_empty() {
            let bindings = if is_import {
                &mut self.import_bindings_by_owner_name
            } else {
                &mut self.local_bindings_by_owner_name
            };
            bindings.insert((owner.id.clone(), name));
        }
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
                GraphEdgeKind::Imports => self.resolve_import(&pending),
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
        let simple = simple_name(&pending.label);
        let segments = qualified_segments(&pending.label);

        let scoped = if segments.len() == 1 {
            self.nearest_scope_candidates(&pending.from, &simple, false, true)
        } else if is_self_qualifier(&segments) {
            self.nearest_scope_candidates(&pending.from, &simple, true, false)
        } else {
            self.named_owner_candidates(&pending.from, &segments, &simple)
        };
        if let Some(scoped) = scoped {
            return self.classify_reference(pending, scoped);
        }

        if segments.len() > 1 {
            if self.qualifier_is_locally_bound(&pending.from, &segments) {
                return self.classify_reference(pending, Vec::new());
            }
            let module_candidates = self.module_qualified_candidates(&segments, &simple);
            if !module_candidates.is_empty() {
                return self.classify_reference(pending, module_candidates);
            }
            if self.qualifier_is_import_bound(&pending.from, &segments) {
                return self.classify_reference(pending, Vec::new());
            }
            if let Some(candidates) = self
                .qualified_symbols
                .get(&normalize_qualified_label(&pending.label))
                .cloned()
            {
                return self.classify_reference(pending, candidates);
            }
        }

        self.classify_reference(pending, Vec::new())
    }

    fn resolve_import(&mut self, pending: &PendingEdge) -> (String, GraphConfidence) {
        let candidates = import_keys(&pending.path, pending.lang, &pending.label)
            .into_iter()
            .filter_map(|key| self.files_by_module_key.get(&key))
            .flatten()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        self.classify_reference(pending, candidates)
    }

    fn classify_reference(
        &mut self,
        pending: &PendingEdge,
        mut candidates: Vec<String>,
    ) -> (String, GraphConfidence) {
        candidates.sort();
        candidates.dedup();
        match candidates.as_slice() {
            [target] => (target.clone(), GraphConfidence::Syntactic),
            [] => (
                self.external_node(pending.lang, &pending.label, pending.kind),
                GraphConfidence::Syntactic,
            ),
            _ => (
                self.external_node(pending.lang, &pending.label, pending.kind),
                GraphConfidence::Ambiguous,
            ),
        }
    }

    fn nearest_scope_candidates(
        &self,
        from: &str,
        simple: &str,
        include_type_scopes: bool,
        respect_bindings: bool,
    ) -> Option<Vec<String>> {
        let mut scope = Some(from);
        while let Some(id) = scope {
            if respect_bindings
                && (self
                    .local_bindings_by_owner_name
                    .contains(&(id.to_string(), simple.to_string()))
                    || self
                        .import_bindings_by_owner_name
                        .contains(&(id.to_string(), simple.to_string())))
            {
                return Some(Vec::new());
            }
            let searchable = self
                .nodes
                .get(id)
                .is_none_or(|node| include_type_scopes || !is_type_scope(node.kind));
            if searchable
                && let Some(candidates) = self
                    .symbols_by_owner_name
                    .get(&(id.to_string(), simple.to_string()))
            {
                return Some(candidates.clone());
            }
            scope = self.parent_by_symbol.get(id).map(String::as_str);
        }
        None
    }

    fn named_owner_candidates(
        &self,
        from: &str,
        segments: &[String],
        simple: &str,
    ) -> Option<Vec<String>> {
        let qualifier = segments.get(segments.len().saturating_sub(2))?;
        let mut scope = Some(from);
        while let Some(id) = scope {
            if self.nodes.get(id).is_some_and(|node| {
                node.name == *qualifier
                    && matches!(
                        node.kind,
                        GraphNodeKind::Class
                            | GraphNodeKind::Struct
                            | GraphNodeKind::Trait
                            | GraphNodeKind::Interface
                            | GraphNodeKind::Module
                            | GraphNodeKind::Namespace
                    )
            }) && let Some(candidates) = self
                .symbols_by_owner_name
                .get(&(id.to_string(), simple.to_string()))
            {
                return Some(candidates.clone());
            }
            scope = self.parent_by_symbol.get(id).map(String::as_str);
        }
        None
    }

    fn qualifier_is_locally_bound(&self, from: &str, segments: &[String]) -> bool {
        self.qualifier_has_binding(from, segments, &self.local_bindings_by_owner_name)
    }

    fn qualifier_is_import_bound(&self, from: &str, segments: &[String]) -> bool {
        self.qualifier_has_binding(from, segments, &self.import_bindings_by_owner_name)
    }

    fn qualifier_has_binding(
        &self,
        from: &str,
        segments: &[String],
        bindings: &BTreeSet<(String, String)>,
    ) -> bool {
        let Some(qualifier) = segments.first() else {
            return false;
        };
        let mut scope = Some(from);
        while let Some(id) = scope {
            if bindings.contains(&(id.to_string(), qualifier.to_string())) {
                return true;
            }
            scope = self.parent_by_symbol.get(id).map(String::as_str);
        }
        false
    }

    fn module_qualified_candidates(&self, segments: &[String], simple: &str) -> Vec<String> {
        let file_ids = qualifier_keys(segments)
            .into_iter()
            .filter_map(|key| self.files_by_module_key.get(&key))
            .flatten()
            .cloned()
            .collect::<BTreeSet<_>>();
        self.local_symbols
            .get(simple)
            .into_iter()
            .flatten()
            .filter(|candidate| {
                self.root_file_id(candidate)
                    .is_some_and(|file_id| file_ids.contains(file_id))
            })
            .cloned()
            .collect()
    }

    fn root_file_id<'a>(&'a self, symbol: &'a str) -> Option<&'a str> {
        let mut current = symbol;
        loop {
            let node = self.nodes.get(current)?;
            if node.kind == GraphNodeKind::File {
                return Some(current);
            }
            current = self.parent_by_symbol.get(current)?;
        }
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
        "In deslop.graph/1, resolved reference authority is not available yet: contains edges are resolved syntax ownership, while calls/imports/inherits are syntactic or ambiguous.",
        "A syntactic edge points to the best scoped candidate or an unresolved placeholder; it is not name-resolution proof. Ambiguous edges retain no candidate list in graph/1.",
        "This graph is deterministic tree-sitter evidence, not a behavior-preservation proof; run verify/apply before writing.",
    ]
    .into_iter()
    .map(ToString::to_string)
    .collect()
}

fn qualified_name(owner: &Owner, name: &str) -> String {
    format!("{}::{name}", owner.name)
}

fn compact_label(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn push_candidate<K: Ord>(map: &mut BTreeMap<K, Vec<String>>, key: K, id: &str) {
    let candidates = map.entry(key).or_default();
    if !candidates.iter().any(|candidate| candidate == id) {
        candidates.push(id.to_string());
    }
}

fn normalize_qualified_label(label: &str) -> String {
    qualified_segments(label).join("::")
}

fn qualified_segments(label: &str) -> Vec<String> {
    label
        .replace("::", "/")
        .replace('.', "/")
        .split('/')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn qualifier_keys(segments: &[String]) -> Vec<String> {
    let qualifier = &segments[..segments.len().saturating_sub(1)];
    let qualifier = if qualifier
        .first()
        .is_some_and(|part| matches!(part.as_str(), "crate" | "self" | "super"))
    {
        &qualifier[1..]
    } else {
        qualifier
    };
    if qualifier.is_empty() {
        return Vec::new();
    }
    let mut keys = vec![
        qualifier.join("::"),
        qualifier.join("."),
        qualifier.join("/"),
        qualifier.last().cloned().unwrap_or_default(),
    ];
    keys.sort();
    keys.dedup();
    keys
}

fn is_self_qualifier(segments: &[String]) -> bool {
    segments
        .get(segments.len().saturating_sub(2))
        .is_some_and(|part| matches!(part.as_str(), "self" | "this" | "Self"))
}

fn is_type_scope(kind: GraphNodeKind) -> bool {
    matches!(
        kind,
        GraphNodeKind::Class
            | GraphNodeKind::Struct
            | GraphNodeKind::Trait
            | GraphNodeKind::Interface
    )
}
