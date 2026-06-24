use std::path::Path;

use deslop_core::Lang;

use crate::types::{GraphConfidence, GraphEdgeKind, GraphNodeKind};

pub(crate) fn file_id(path: &Path) -> String {
    format!("file:{}", normalized_path(path))
}

pub(crate) fn symbol_id(path: &Path, kind: GraphNodeKind, name: &str, start_byte: usize) -> String {
    format!(
        "sym:{}#{}:{}@{}",
        normalized_path(path),
        node_kind_label(kind),
        sanitize_id(name),
        start_byte
    )
}

pub(crate) fn external_id(lang: Lang, edge_kind: GraphEdgeKind, label: &str) -> String {
    format!(
        "external:{}:{}:{}",
        lang,
        edge_kind_label(edge_kind),
        sanitize_id(label)
    )
}

pub(crate) fn normalized_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_string()
}

pub(crate) fn sanitize_id(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | ':') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

pub(crate) fn simple_name(label: &str) -> String {
    let cleaned = label
        .trim()
        .trim_matches(|ch| matches!(ch, '(' | ')' | '[' | ']' | '{' | '}' | '"' | '\'' | ';'));
    cleaned
        .split([':', '.', '/', '<', '>', ' ', '\t', '\n'])
        .rfind(|part| !part.is_empty())
        .unwrap_or(cleaned)
        .trim_end_matches('!')
        .to_string()
}

pub(crate) fn module_keys(path: &Path, lang: Lang) -> Vec<String> {
    let normalized = normalized_path(path);
    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string();
    let without_ext = normalized
        .rsplit_once('.')
        .map(|(base, _)| base.to_string())
        .unwrap_or(normalized);
    let dotted = without_ext.replace('/', ".");
    let slashed = without_ext;
    let mut keys = vec![stem, dotted.clone(), slashed.clone()];
    match lang {
        Lang::Rust => keys.push(dotted.replace('.', "::")),
        Lang::Clojure | Lang::Python | Lang::Julia | Lang::JavaScript | Lang::TypeScript => {
            keys.push(dotted)
        }
        Lang::Generic => {}
    }
    keys.sort();
    keys.dedup();
    keys
}

pub(crate) fn import_keys(path: &Path, lang: Lang, label: &str) -> Vec<String> {
    let cleaned = label
        .trim()
        .trim_matches(|ch| matches!(ch, '"' | '\'' | ';'));
    let mut keys = vec![cleaned.to_string()];
    keys.push(cleaned.replace("::", "."));
    keys.push(cleaned.replace('.', "/"));
    if matches!(lang, Lang::JavaScript | Lang::TypeScript)
        && cleaned.starts_with('.')
        && let Some(parent) = path.parent()
    {
        let joined = parent.join(cleaned);
        keys.extend(module_keys(&joined, lang));
    }
    if matches!(lang, Lang::Rust) {
        keys.push(simple_name(cleaned));
    }
    keys.sort();
    keys.dedup();
    keys
}

pub(crate) fn node_kind_label(kind: GraphNodeKind) -> &'static str {
    match kind {
        GraphNodeKind::File => "file",
        GraphNodeKind::Module => "module",
        GraphNodeKind::Namespace => "namespace",
        GraphNodeKind::Function => "function",
        GraphNodeKind::Method => "method",
        GraphNodeKind::Class => "class",
        GraphNodeKind::Struct => "struct",
        GraphNodeKind::Enum => "enum",
        GraphNodeKind::Trait => "trait",
        GraphNodeKind::Interface => "interface",
        GraphNodeKind::Constant => "constant",
        GraphNodeKind::Variable => "variable",
        GraphNodeKind::ExternalSymbol => "external-symbol",
    }
}

pub(crate) fn edge_kind_label(kind: GraphEdgeKind) -> &'static str {
    match kind {
        GraphEdgeKind::Contains => "contains",
        GraphEdgeKind::Imports => "imports",
        GraphEdgeKind::Calls => "calls",
        GraphEdgeKind::Inherits => "inherits",
    }
}

pub(crate) fn confidence_label(confidence: GraphConfidence) -> &'static str {
    match confidence {
        GraphConfidence::Resolved => "resolved",
        GraphConfidence::Syntactic => "syntactic",
        GraphConfidence::Ambiguous => "ambiguous",
        GraphConfidence::External => "external",
    }
}
