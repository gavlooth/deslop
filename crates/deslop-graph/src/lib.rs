use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use deslop_lang::Registry;
use deslop_parse::SourceFile;
use ignore::WalkBuilder;

mod builder;
mod extract;
mod ids;
mod render;
mod types;

pub use render::{render_dot, render_json};
pub use types::{
    DependencyGraph, GraphConfidence, GraphConfig, GraphEdge, GraphEdgeKind, GraphNode,
    GraphNodeKind, GraphNotice, GraphSummary,
};

pub fn graph_paths(paths: &[PathBuf], config: GraphConfig) -> Result<DependencyGraph> {
    let registry = Registry::default();
    let mut paths = collect_supported_paths(paths, &registry)?;
    paths.sort();
    paths.dedup();

    let mut builder = builder::GraphBuilder::new(config);
    for path in &paths {
        builder.index_file_path(path, &registry);
    }

    for path in paths {
        let source = SourceFile::read(&path)?;
        builder.add_source(&source, &registry)?;
    }

    Ok(builder.finish())
}

fn collect_supported_paths(paths: &[PathBuf], registry: &Registry) -> Result<Vec<PathBuf>> {
    let roots = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths.to_vec()
    };
    let mut out = Vec::new();
    for path in roots {
        collect_supported_path(&mut out, &path, registry)?;
    }
    Ok(out)
}

fn collect_supported_path(out: &mut Vec<PathBuf>, path: &Path, registry: &Registry) -> Result<()> {
    if path.is_file() {
        if registry.supported_pack_for_path(path).is_some() {
            out.push(path.to_path_buf());
        }
        return Ok(());
    }

    let walker = WalkBuilder::new(path)
        .hidden(false)
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !matches!(name.as_ref(), ".git" | ".jj" | "target" | "__pycache__")
        })
        .build();

    for entry in walker {
        let entry = entry.with_context(|| format!("failed to walk {}", path.display()))?;
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let path = entry.into_path();
        if registry.supported_pack_for_path(&path).is_some() {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SCHEMA;
    use deslop_core::{Lang, Span};

    #[test]
    fn rust_graph_resolves_unique_local_call() {
        let temp = tempfile::tempdir().expect("tempdir");
        let lib = temp.path().join("lib.rs");
        let util = temp.path().join("util.rs");
        std::fs::write(
            &lib,
            "mod util;\n\npub fn run() {\n    util::helper();\n}\n",
        )
        .expect("write lib");
        std::fs::write(&util, "pub fn helper() {}\n").expect("write util");

        let graph = graph_paths(&[temp.path().to_path_buf()], GraphConfig::default()).unwrap();
        assert_eq!(graph.schema, SCHEMA);
        assert!(
            has_node(&graph, GraphNodeKind::Function, "run"),
            "{graph:#?}"
        );
        assert!(
            has_edge(
                &graph,
                GraphEdgeKind::Calls,
                GraphConfidence::Resolved,
                "util::helper",
            ),
            "{graph:#?}"
        );
    }

    #[test]
    fn python_graph_emits_external_call_node() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("sample.py");
        std::fs::write(&path, "def run():\n    print('x')\n").expect("write");

        let graph = graph_paths(&[path], GraphConfig::default()).unwrap();
        assert!(
            has_node(&graph, GraphNodeKind::ExternalSymbol, "print"),
            "{graph:#?}"
        );
        assert!(
            has_edge(
                &graph,
                GraphEdgeKind::Calls,
                GraphConfidence::External,
                "print",
            ),
            "{graph:#?}"
        );
    }

    #[test]
    fn dot_render_includes_edge_labels() {
        let graph = DependencyGraph {
            schema: SCHEMA.to_string(),
            summary: GraphSummary {
                files: 0,
                symbols: 0,
                external_symbols: 0,
                edges: 1,
                resolved_edges: 1,
                ambiguous_edges: 0,
                external_edges: 0,
            },
            agent_notes: Vec::new(),
            nodes: vec![
                test_node("a", GraphNodeKind::File),
                test_node("b", GraphNodeKind::Function),
            ],
            edges: vec![GraphEdge {
                kind: GraphEdgeKind::Contains,
                from: "a".to_string(),
                to: "b".to_string(),
                confidence: GraphConfidence::Resolved,
                label: Some("b".to_string()),
                span: Some(Span::new(1, 1, 0, 0)),
            }],
            notices: Vec::new(),
        };
        assert!(render_dot(&graph).contains("contains: b"));
    }

    fn test_node(id: &str, kind: GraphNodeKind) -> GraphNode {
        GraphNode {
            id: id.to_string(),
            kind,
            lang: Lang::Rust,
            path: None,
            name: id.to_string(),
            qualified_name: id.to_string(),
            span: None,
            signature: None,
        }
    }

    fn has_node(graph: &DependencyGraph, kind: GraphNodeKind, name: &str) -> bool {
        graph
            .nodes
            .iter()
            .any(|node| node.kind == kind && node.name == name)
    }

    fn has_edge(
        graph: &DependencyGraph,
        kind: GraphEdgeKind,
        confidence: GraphConfidence,
        label: &str,
    ) -> bool {
        graph.edges.iter().any(|edge| {
            edge.kind == kind
                && edge.confidence == confidence
                && edge.label.as_deref() == Some(label)
        })
    }
}
