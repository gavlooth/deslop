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
    let cleaned = import_module_label(lang, label);
    let mut keys = vec![cleaned.clone()];
    keys.push(cleaned.replace("::", "."));
    keys.push(cleaned.replace('.', "/"));
    if matches!(lang, Lang::JavaScript | Lang::TypeScript)
        && cleaned.starts_with('.')
        && let Some(parent) = path.parent()
    {
        let joined = parent.join(cleaned.trim_start_matches("./"));
        keys.extend(module_keys(&joined, lang));
    }
    if matches!(lang, Lang::Rust) {
        let stripped = cleaned
            .strip_prefix("crate::")
            .or_else(|| cleaned.strip_prefix("self::"))
            .or_else(|| cleaned.strip_prefix("super::"))
            .unwrap_or(&cleaned);
        keys.push(stripped.to_string());
        if let Some((parent, _)) = stripped.rsplit_once("::") {
            keys.push(parent.to_string());
            keys.push(simple_name(parent));
        } else {
            keys.push(simple_name(stripped));
        }
    }
    keys.sort();
    keys.dedup();
    keys
}

fn import_module_label(lang: Lang, label: &str) -> String {
    let cleaned = label
        .trim()
        .trim_matches(|ch| matches!(ch, '"' | '\'' | ';'));
    match lang {
        Lang::Rust => cleaned
            .split_once(" as ")
            .map_or(cleaned, |(source, _)| source)
            .to_string(),
        Lang::Python => {
            if let Some(rest) = cleaned.strip_prefix("from ") {
                rest.split_once(" import ")
                    .map_or(rest, |(module, _)| module)
                    .to_string()
            } else {
                cleaned
                    .strip_prefix("import ")
                    .unwrap_or(cleaned)
                    .split([',', ' '])
                    .next()
                    .unwrap_or(cleaned)
                    .to_string()
            }
        }
        Lang::Julia => {
            let source = cleaned
                .split_once(':')
                .map_or(cleaned, |(module, _)| module);
            source
                .split_once(" as ")
                .map_or(source, |(module, _)| module)
                .trim_start_matches('.')
                .to_string()
        }
        Lang::Clojure => cleaned
            .split_whitespace()
            .collect::<Vec<_>>()
            .windows(2)
            .find(|pair| matches!(pair[0], ":require" | "require"))
            .map(|pair| pair[1])
            .unwrap_or(cleaned)
            .to_string(),
        Lang::JavaScript | Lang::TypeScript | Lang::Generic => cleaned.to_string(),
    }
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
