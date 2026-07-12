use anyhow::Result;

use crate::ids::{confidence_label, edge_kind_label};
use crate::types::{DependencyGraph, GraphConfidence, GraphEdge, GraphNodeKind};

pub fn render_json(graph: &DependencyGraph) -> Result<String> {
    Ok(format!("{}\n", serde_json::to_string_pretty(graph)?))
}

pub fn render_dot(graph: &DependencyGraph) -> String {
    let mut out = String::from("digraph deslop_graph {\n  rankdir=LR;\n");
    for notice in &graph.notices {
        out.push_str(&format!(
            "  // {}: {}\n",
            escape_dot(&notice.path.display().to_string()),
            escape_dot(&notice.message)
        ));
    }
    for node in &graph.nodes {
        let label = match &node.signature {
            Some(signature) if !signature.is_empty() => format!("{}\\n{}", node.name, signature),
            _ => node.name.clone(),
        };
        out.push_str(&format!(
            "  \"{}\" [label=\"{}\", shape={}];\n",
            escape_dot(&node.id),
            escape_dot(&label),
            dot_shape(node.kind)
        ));
    }
    for edge in &graph.edges {
        let mut label = edge_label(edge);
        if edge.confidence != GraphConfidence::Resolved {
            label.push_str(&format!(" ({})", confidence_label(edge.confidence)));
        }
        out.push_str(&format!(
            "  \"{}\" -> \"{}\" [label=\"{}\"];\n",
            escape_dot(&edge.from),
            escape_dot(&edge.to),
            escape_dot(&label)
        ));
    }
    out.push_str("}\n");
    out
}

fn escape_dot(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn dot_shape(kind: GraphNodeKind) -> &'static str {
    match kind {
        GraphNodeKind::File => "folder",
        GraphNodeKind::ExternalSymbol => "box",
        GraphNodeKind::Class
        | GraphNodeKind::Struct
        | GraphNodeKind::Enum
        | GraphNodeKind::Trait
        | GraphNodeKind::Interface => "component",
        _ => "ellipse",
    }
}

fn edge_label(edge: &GraphEdge) -> String {
    match &edge.label {
        Some(label) => format!("{}: {label}", edge_kind_label(edge.kind)),
        None => edge_kind_label(edge.kind).to_string(),
    }
}
