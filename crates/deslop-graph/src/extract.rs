use std::collections::BTreeSet;

use crate::builder::{GraphBuilder, GraphFile, OwnedNode as Node};
use crate::types::{GraphEdgeKind, GraphNodeKind, Owner, SymbolDef};
use deslop_core::{Lang, Span};

pub(crate) fn extract_source(
    builder: &mut GraphBuilder,
    source: &GraphFile<'_>,
    root: Node<'_>,
    file_id: String,
) {
    let owner = Owner {
        id: file_id,
        kind: GraphNodeKind::File,
        name: source.path.display().to_string(),
    };
    let mut extractor = SourceExtractor {
        builder,
        source,
        owners: vec![owner],
    };
    extractor.visit(root);
}

struct SourceExtractor<'a> {
    builder: &'a mut GraphBuilder,
    source: &'a GraphFile<'a>,
    owners: Vec<Owner>,
}

impl SourceExtractor<'_> {
    fn visit(&mut self, node: Node<'_>) {
        let current_owner = self.owner().clone();
        let is_import_binding = is_import_binding_node(self.source.lang, node, self.source);
        for name in opaque_binding_names(self.source.lang, node, self.source) {
            self.builder
                .add_binding(&current_owner, name, is_import_binding);
        }

        if let Some(label) = import_label(self.source.lang, node, self.source) {
            self.add_extracted_edge(GraphEdgeKind::Imports, node, label);
        }

        if self.builder.include_calls()
            && let Some(label) = call_label(self.source.lang, node, self.source)
        {
            self.add_extracted_edge(GraphEdgeKind::Calls, node, label);
        }

        let inheritance = inheritance_labels(self.source.lang, node, self.source);

        if let Some(def) = symbol_def(self.source.lang, node, self.source, self.owner()) {
            let current_owner = self.owner().clone();
            let owner = self
                .builder
                .add_symbol_node(self.source, &current_owner, node, def);
            self.owners.push(owner);
            for label in inheritance {
                self.add_extracted_edge(GraphEdgeKind::Inherits, node, label);
            }
            self.visit_children(node);
            self.owners.pop();
            return;
        }

        for label in inheritance {
            self.add_extracted_edge(GraphEdgeKind::Inherits, node, label);
        }

        self.visit_children(node);
    }

    fn add_extracted_edge(&mut self, kind: GraphEdgeKind, node: Node<'_>, label: String) {
        let owner = self.owner().clone();
        self.builder
            .add_pending_edge(kind, &owner, self.source, node, label);
    }

    fn visit_children(&mut self, node: Node<'_>) {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.visit(child);
        }
    }

    fn owner(&self) -> &Owner {
        self.owners.last().expect("source extractor owner")
    }
}

fn opaque_binding_names(lang: Lang, node: Node<'_>, source: &GraphFile<'_>) -> Vec<String> {
    if is_import_binding_node(lang, node, source) {
        return import_binding_names(lang, node_text(source, node));
    }
    if lang == Lang::Clojure
        && matches!(node.kind(), "vec_lit" | "vector_lit")
        && let Some(names) = clojure_binding_names(node, source)
    {
        return names;
    }

    let binding = match lang {
        Lang::Rust if node.kind() == "let_declaration" => node.child_by_field_name("pattern"),
        Lang::Python
            if matches!(
                node.kind(),
                "assignment" | "named_expression" | "for_statement" | "with_item"
            ) =>
        {
            node.child_by_field_name("left")
                .or_else(|| node.child_by_field_name("name"))
                .or_else(|| node.child_by_field_name("alias"))
        }
        Lang::JavaScript | Lang::TypeScript if node.kind() == "variable_declarator" => {
            if node.child_by_field_name("value").is_some_and(|value| {
                matches!(value.kind(), "arrow_function" | "function_expression")
            }) {
                None
            } else {
                node.child_by_field_name("name")
            }
        }
        Lang::JavaScript | Lang::TypeScript
            if matches!(node.kind(), "catch_clause" | "for_in_statement") =>
        {
            node.child_by_field_name("parameter")
                .or_else(|| node.child_by_field_name("left"))
        }
        Lang::Julia if matches!(node.kind(), "assignment" | "for_binding") => node
            .child_by_field_name("left")
            .or_else(|| node.child_by_field_name("name"))
            .or_else(|| node.named_child(0)),
        _ => None,
    };
    if let Some(binding) = binding {
        return pattern_names(binding, source);
    }

    if is_parameter_container(lang, node.kind()) {
        let mut names = BTreeSet::new();
        let mut cursor = node.walk();
        for parameter in node.named_children(&mut cursor) {
            let binding = parameter
                .child_by_field_name("pattern")
                .or_else(|| parameter.child_by_field_name("name"))
                .unwrap_or(parameter);
            let name = if identifier_kind(binding.kind()) {
                Some(node_text(source, binding).trim().to_string())
            } else {
                first_identifier(binding, source)
            };
            if let Some(name) = name {
                names.insert(name);
            }
        }
        return names.into_iter().collect();
    }

    Vec::new()
}

fn clojure_binding_names(node: Node<'_>, source: &GraphFile<'_>) -> Option<Vec<String>> {
    let parent = node.parent()?;
    if parent.kind() != "list_lit" {
        return None;
    }
    let head = clojure_tokens(node_text(source, parent))
        .into_iter()
        .next()?;
    let tokens = clojure_tokens(node_text(source, node));
    let names = match head.as_str() {
        "defn" | "defn-" | "defmacro" | "fn" | "defmethod" => tokens,
        "let" | "letfn" | "loop" | "for" | "doseq" | "binding" | "with-open" | "if-let"
        | "when-let" => tokens.into_iter().step_by(2).collect(),
        _ => return None,
    };
    Some(
        names
            .into_iter()
            .filter(|name| valid_binding_token(name) && name != "_")
            .collect(),
    )
}

fn is_import_binding_node(lang: Lang, node: Node<'_>, source: &GraphFile<'_>) -> bool {
    is_import_node(lang, node.kind())
        && (lang != Lang::Clojure || import_label(lang, node, source).is_some())
}

fn is_import_node(lang: Lang, kind: &str) -> bool {
    match lang {
        Lang::Rust => matches!(kind, "use_declaration" | "extern_crate_declaration"),
        Lang::Python => matches!(kind, "import_statement" | "import_from_statement"),
        Lang::JavaScript | Lang::TypeScript => matches!(kind, "import_statement"),
        Lang::Julia => matches!(kind, "import_statement" | "using_statement"),
        Lang::Clojure => kind == "list_lit",
        Lang::Generic => false,
    }
}

fn is_parameter_container(lang: Lang, kind: &str) -> bool {
    match lang {
        Lang::Rust | Lang::Python | Lang::Julia => kind == "parameters",
        Lang::JavaScript | Lang::TypeScript => kind == "formal_parameters",
        Lang::Clojure | Lang::Generic => false,
    }
}

fn pattern_names(node: Node<'_>, source: &GraphFile<'_>) -> Vec<String> {
    let mut names = BTreeSet::new();
    collect_pattern_names(node, source, &mut names);
    names.into_iter().collect()
}

fn collect_pattern_names(node: Node<'_>, source: &GraphFile<'_>, names: &mut BTreeSet<String>) {
    if matches!(node.kind(), "identifier" | "identifier_lit" | "symbol") {
        let name = node_text(source, node).trim().to_string();
        if !name.is_empty() && name != "_" {
            names.insert(name);
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_pattern_names(child, source, names);
    }
}

fn import_binding_names(lang: Lang, text: &str) -> Vec<String> {
    if matches!(lang, Lang::JavaScript | Lang::TypeScript)
        && text
            .trim_start()
            .strip_prefix("import")
            .is_some_and(|rest| rest.trim_start().starts_with(['"', '\'']))
    {
        return Vec::new();
    }
    let normalized = text.replace(
        [',', ';', '(', ')', '[', ']', '{', '}', '"', '\'', ':'],
        " ",
    );
    let tokens = normalized.split_whitespace().collect::<Vec<_>>();
    let mut names = BTreeSet::new();

    for pair in tokens.windows(2) {
        if matches!(pair[0], "as" | ":as") && valid_binding_token(pair[1]) {
            names.insert(pair[1].to_string());
        }
    }

    match lang {
        Lang::Rust => {
            if !tokens.contains(&"as")
                && let Some(name) = tokens.iter().rev().find(|token| valid_binding_token(token))
            {
                names.insert(simple_path_component(name));
            }
        }
        Lang::Python => {
            if let Some(import_at) = tokens.iter().position(|token| *token == "import") {
                for name in tokens.iter().skip(import_at + 1) {
                    if *name == "as" {
                        break;
                    }
                    if valid_binding_token(name) {
                        names.insert(simple_path_component(name));
                    }
                }
            }
        }
        Lang::JavaScript | Lang::TypeScript => {
            for name in tokens.iter().skip(1).take_while(|token| **token != "from") {
                if !["as", "*"].contains(name) && valid_binding_token(name) {
                    names.insert(simple_path_component(name));
                }
            }
        }
        Lang::Julia => {
            for name in tokens.iter().skip(1) {
                if !["as", "using", "import"].contains(name) && valid_binding_token(name) {
                    names.insert(simple_path_component(name));
                }
            }
        }
        Lang::Clojure => {
            for pair in tokens.windows(2) {
                if pair[0] == "as" && valid_binding_token(pair[1]) {
                    names.insert(pair[1].to_string());
                }
            }
        }
        Lang::Generic => {}
    }

    names
        .into_iter()
        .filter(|name| {
            !matches!(
                name.as_str(),
                "use" | "extern" | "crate" | "from" | "import"
            )
        })
        .collect()
}

fn valid_binding_token(token: &str) -> bool {
    !token.is_empty()
        && token != "*"
        && token
            .chars()
            .any(|character| character.is_alphanumeric() || character == '_')
}

fn simple_path_component(token: &str) -> String {
    token
        .split([':', '.', '/'])
        .rfind(|part| !part.is_empty())
        .unwrap_or(token)
        .to_string()
}

fn symbol_def(
    lang: Lang,
    node: Node<'_>,
    source: &GraphFile<'_>,
    owner: &Owner,
) -> Option<SymbolDef> {
    match lang {
        Lang::Rust => rust_symbol_def(node, source, owner),
        Lang::Python => python_symbol_def(node, source, owner),
        Lang::JavaScript | Lang::TypeScript => js_symbol_def(node, source, owner),
        Lang::Julia => julia_symbol_def(node, source),
        Lang::Clojure => clojure_symbol_def(node, source),
        Lang::Generic => None,
    }
}

fn rust_symbol_def(node: Node<'_>, source: &GraphFile<'_>, owner: &Owner) -> Option<SymbolDef> {
    let kind = match node.kind() {
        "function_item" if matches!(owner.kind, GraphNodeKind::Struct | GraphNodeKind::Trait) => {
            GraphNodeKind::Method
        }
        "function_item" => GraphNodeKind::Function,
        "impl_item" => GraphNodeKind::Struct,
        "struct_item" => GraphNodeKind::Struct,
        "enum_item" => GraphNodeKind::Enum,
        "trait_item" => GraphNodeKind::Trait,
        "mod_item" => GraphNodeKind::Module,
        "const_item" | "static_item" => GraphNodeKind::Constant,
        "type_item" => GraphNodeKind::Variable,
        _ => return None,
    };
    let name = if node.kind() == "impl_item" {
        node.child_by_field_name("trait")
            .or_else(|| node.child_by_field_name("type"))
            .map(|child| compact_label(node_text(source, child)))
            .or_else(|| first_identifier(node, source))
    } else {
        name_field(node, source)
    }?;
    Some(SymbolDef { kind, name })
}

fn python_symbol_def(node: Node<'_>, source: &GraphFile<'_>, owner: &Owner) -> Option<SymbolDef> {
    let kind = match node.kind() {
        "function_definition" if owner.kind == GraphNodeKind::Class => GraphNodeKind::Method,
        "function_definition" => GraphNodeKind::Function,
        "class_definition" => GraphNodeKind::Class,
        _ => return None,
    };
    Some(SymbolDef {
        kind,
        name: name_field(node, source)?,
    })
}

fn js_symbol_def(node: Node<'_>, source: &GraphFile<'_>, owner: &Owner) -> Option<SymbolDef> {
    let kind = match node.kind() {
        "function_declaration" => GraphNodeKind::Function,
        "method_definition" => GraphNodeKind::Method,
        "class_declaration" => GraphNodeKind::Class,
        "interface_declaration" => GraphNodeKind::Interface,
        "lexical_declaration" | "variable_declaration" => {
            return variable_function_def(node, source);
        }
        "arrow_function" if owner.kind == GraphNodeKind::Class => GraphNodeKind::Method,
        "arrow_function" => return None,
        _ => return None,
    };
    Some(SymbolDef {
        kind,
        name: name_field(node, source).or_else(|| first_identifier(node, source))?,
    })
}

fn variable_function_def(node: Node<'_>, source: &GraphFile<'_>) -> Option<SymbolDef> {
    let text = node_text(source, node);
    if !(text.contains("=>") || text.contains("function")) {
        return None;
    }
    first_identifier(node, source).map(|name| SymbolDef {
        kind: GraphNodeKind::Function,
        name,
    })
}

fn julia_symbol_def(node: Node<'_>, source: &GraphFile<'_>) -> Option<SymbolDef> {
    let kind = match node.kind() {
        "function_definition" => GraphNodeKind::Function,
        "struct_definition" => GraphNodeKind::Struct,
        "module_definition" => GraphNodeKind::Module,
        "macro_definition" => GraphNodeKind::Function,
        _ => return None,
    };
    let name = name_field(node, source).or_else(|| first_identifier(node, source))?;
    Some(SymbolDef { kind, name })
}

fn clojure_symbol_def(node: Node<'_>, source: &GraphFile<'_>) -> Option<SymbolDef> {
    if node.kind() != "list_lit" {
        return None;
    }
    let tokens = clojure_tokens(node_text(source, node));
    let head = tokens.first()?.as_str();
    let kind = match head {
        "ns" => GraphNodeKind::Namespace,
        "defn" | "defn-" | "defmacro" | "defmulti" | "defmethod" => GraphNodeKind::Function,
        "defrecord" | "deftype" => GraphNodeKind::Struct,
        "defprotocol" | "definterface" => GraphNodeKind::Interface,
        "def" | "defonce" => GraphNodeKind::Variable,
        _ => return None,
    };
    let name = tokens.get(1)?.clone();
    Some(SymbolDef { kind, name })
}

fn import_label(lang: Lang, node: Node<'_>, source: &GraphFile<'_>) -> Option<String> {
    match lang {
        Lang::Rust if matches!(node.kind(), "use_declaration" | "extern_crate_declaration") => {
            Some(strip_keywords(
                node_text(source, node),
                &["use", "extern crate"],
                &[";"],
            ))
        }
        Lang::Python if matches!(node.kind(), "import_statement" | "import_from_statement") => {
            Some(compact_label(node_text(source, node)))
        }
        Lang::JavaScript | Lang::TypeScript
            if matches!(
                node.kind(),
                "import_statement" | "import_declaration" | "export_statement"
            ) =>
        {
            first_string_literal(node, source)
                .or_else(|| Some(compact_label(node_text(source, node))))
        }
        Lang::Julia if matches!(node.kind(), "import_statement" | "using_statement") => Some(
            strip_keywords(node_text(source, node), &["import", "using"], &[]),
        ),
        Lang::Clojure if node.kind() == "list_lit" => {
            let tokens = clojure_tokens(node_text(source, node));
            if matches!(
                tokens.first().map(String::as_str),
                Some("ns" | "require" | "import")
            ) {
                Some(tokens.join(" "))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn call_label(lang: Lang, node: Node<'_>, source: &GraphFile<'_>) -> Option<String> {
    match lang {
        Lang::Rust if node.kind() == "call_expression" => call_function_label(node, source),
        Lang::Rust if node.kind() == "macro_invocation" => {
            name_field(node, source).or_else(|| first_identifier(node, source))
        }
        Lang::Python if node.kind() == "call" => call_function_label(node, source),
        Lang::JavaScript | Lang::TypeScript
            if matches!(node.kind(), "call_expression" | "new_expression") =>
        {
            call_function_label(node, source)
        }
        Lang::Julia if node.kind() == "call_expression" => {
            call_function_label(node, source).or_else(|| first_identifier(node, source))
        }
        Lang::Clojure if node.kind() == "list_lit" => clojure_call_label(node, source),
        _ => None,
    }
}

fn inheritance_labels(lang: Lang, node: Node<'_>, source: &GraphFile<'_>) -> Vec<String> {
    match lang {
        Lang::Python if node.kind() == "class_definition" => node
            .child_by_field_name("superclasses")
            .map(|superclasses| {
                let mut cursor = superclasses.walk();
                superclasses
                    .named_children(&mut cursor)
                    .map(|child| compact_label(node_text(source, child)))
                    .filter(|label| !label.is_empty())
                    .collect()
            })
            .unwrap_or_default(),
        Lang::JavaScript | Lang::TypeScript if node.kind() == "class_declaration" => {
            let text = node_text(source, node);
            text.split_once("extends")
                .map(|(_, tail)| compact_label(tail.split('{').next().unwrap_or(tail)))
                .filter(|label| !label.is_empty())
                .into_iter()
                .collect()
        }
        _ => Vec::new(),
    }
}

fn call_function_label(node: Node<'_>, source: &GraphFile<'_>) -> Option<String> {
    node.child_by_field_name("function")
        .map(|child| compact_label(node_text(source, child)))
        .filter(|label| !label.is_empty())
}

fn clojure_call_label(node: Node<'_>, source: &GraphFile<'_>) -> Option<String> {
    let tokens = clojure_tokens(node_text(source, node));
    let head = tokens.first()?.as_str();
    if clojure_non_call_heads().contains(head) {
        return None;
    }
    Some(head.to_string())
}

fn clojure_non_call_heads() -> &'static BTreeSet<&'static str> {
    static HEADS: std::sync::OnceLock<BTreeSet<&'static str>> = std::sync::OnceLock::new();
    HEADS.get_or_init(|| {
        [
            "ns",
            "require",
            "import",
            "def",
            "defonce",
            "defn",
            "defn-",
            "defmacro",
            "fn",
            "let",
            "letfn",
            "if",
            "when",
            "when-let",
            "when-not",
            "do",
            "loop",
            "recur",
            "case",
            "cond",
            "cond->",
            "cond->>",
            "for",
            "doseq",
            "quote",
            "var",
            ":require",
            ":import",
            ":refer-clojure",
        ]
        .into_iter()
        .collect()
    })
}

fn name_field(node: Node<'_>, source: &GraphFile<'_>) -> Option<String> {
    node.child_by_field_name("name")
        .map(|child| compact_label(node_text(source, child)))
        .filter(|name| !name.is_empty())
}

fn first_identifier(node: Node<'_>, source: &GraphFile<'_>) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if identifier_kind(child.kind()) {
            let value = compact_label(node_text(source, child));
            if !value.is_empty() && !language_keyword(&value) {
                return Some(value);
            }
        }
        if let Some(value) = first_identifier(child, source) {
            return Some(value);
        }
    }
    None
}

fn first_string_literal(node: Node<'_>, source: &GraphFile<'_>) -> Option<String> {
    const QUOTE_BYTE_WIDTH: usize = 1;

    let text = node_text(source, node);
    let quote_start = text.find(['"', '\''])?;
    let quote = text.as_bytes()[quote_start] as char;
    let tail = &text[quote_start + QUOTE_BYTE_WIDTH..];
    let quote_end = tail.find(quote)?;
    Some(tail[..quote_end].to_string())
}

fn identifier_kind(kind: &str) -> bool {
    matches!(
        kind,
        "identifier"
            | "type_identifier"
            | "field_identifier"
            | "property_identifier"
            | "constant"
            | "module_identifier"
            | "scoped_identifier"
            | "symbol"
            | "identifier_lit"
            | "sym_lit"
    )
}

fn language_keyword(value: &str) -> bool {
    matches!(
        value,
        "fn" | "function" | "struct" | "class" | "module" | "impl" | "trait" | "defn" | "def"
    )
}

fn node_text<'a>(source: &'a GraphFile<'_>, node: Node<'_>) -> &'a str {
    source.text.get(node.byte_range()).unwrap_or("")
}

pub(crate) fn span_for_node(source: &GraphFile<'_>, node: Node<'_>) -> Span {
    let start = node.start_byte();
    let end = node.end_byte();
    let end_line_byte = end.saturating_sub(1).max(start);
    Span::new(
        source.line_for_byte(start),
        source.line_for_byte(end_line_byte),
        start,
        end,
    )
}

pub(crate) fn signature_for_node(source: &GraphFile<'_>, node: Node<'_>) -> Option<String> {
    const SIGNATURE_MAX_CHARS: usize = 160;
    const ELLIPSIS_CHARS: usize = 3;

    let text = node_text(source, node);
    let mut signature = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?
        .to_string();
    if signature.len() > SIGNATURE_MAX_CHARS {
        signature.truncate(SIGNATURE_MAX_CHARS - ELLIPSIS_CHARS);
        signature.push_str("...");
    }
    Some(signature)
}

fn compact_label(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_keywords(text: &str, prefixes: &[&str], suffixes: &[&str]) -> String {
    let mut out = text.trim();
    for prefix in prefixes {
        if let Some(stripped) = out.strip_prefix(prefix) {
            out = stripped.trim();
            break;
        }
    }
    for suffix in suffixes {
        if let Some(stripped) = out.strip_suffix(suffix) {
            out = stripped.trim();
            break;
        }
    }
    compact_label(out)
}

fn clojure_tokens(text: &str) -> Vec<String> {
    text.split(|ch: char| {
        ch.is_whitespace()
            || matches!(
                ch,
                '(' | ')' | '[' | ']' | '{' | '}' | '"' | '\'' | '`' | ',' | ';'
            )
    })
    .filter(|part| !part.is_empty())
    .map(ToOwned::to_owned)
    .collect()
}
